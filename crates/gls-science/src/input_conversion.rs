use super::{
    InputKind, NormalizedInput, ScienceOptions, clean_cell, encode_tsv_cell, read_text,
    run_python_script, tokenize, write_json_manifest, write_text,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

const INPUT_RESOLVER_SCRIPT: &str =
    include_str!("../../../python/genoligand_workers/input_resolver.py");
pub(super) const INPUT_CONVERSION_HEADER: &str = "input_id\trecord_id\toriginal_field\tinferred_kind\tcanonical_kind\tcanonical_value\tconversion_status\tconversion_method\tformat\tsource_kind\tsource_path\tcontent_sha256\treason\tartifact\trisk_flags\tstructure_atom_count\tstructure_chain_count\tstructure_residue_count";

#[derive(Clone, Debug)]
pub(super) struct ConvertedInput {
    pub(super) input_id: String,
    pub(super) record_id: String,
    pub(super) original_field: String,
    pub(super) inferred_kind: String,
    pub(super) canonical_kind: Option<InputKind>,
    pub(super) canonical_value: String,
    pub(super) conversion_status: &'static str,
    pub(super) conversion_method: &'static str,
    pub(super) format: String,
    pub(super) source_kind: &'static str,
    pub(super) source_path: String,
    pub(super) content_sha256: String,
    pub(super) structure_atom_count: usize,
    pub(super) structure_chain_count: usize,
    pub(super) structure_residue_count: usize,
    pub(super) reason: String,
    pub(super) artifact: String,
    pub(super) risk_flags: Vec<String>,
    pub(super) file_count: usize,
}

#[derive(Clone, Debug)]
struct RawInput {
    input_id: String,
    original_field: String,
    value: String,
    file_count: usize,
}

#[derive(Clone, Debug)]
struct ResolvedInputRecord {
    input_id: String,
    record_id: String,
    original_field: String,
    source_kind: &'static str,
    source_path: String,
    content_sha256: String,
    format: String,
    content: String,
    structure_atom_count: usize,
    structure_chain_count: usize,
    structure_residue_count: usize,
    file_count: usize,
}

#[derive(Clone, Copy, Debug)]
struct CoordinateSummary {
    atom_count: usize,
    chain_count: usize,
    residue_count: usize,
}

struct ConversionDetails<'a> {
    kind: InputKind,
    canonical_value: String,
    inferred_kind: &'a str,
    method: &'static str,
    format: &'a str,
    artifact: &'a str,
    risk_flag: &'a str,
}

pub(super) fn extract_inputs(options: &ScienceOptions) -> Result<Vec<NormalizedInput>, String> {
    Ok(normalized_inputs_from_conversions(
        &convert_inputs_from_manifest(options)?,
    ))
}

pub(super) fn convert_inputs_from_manifest(
    options: &ScienceOptions,
) -> Result<Vec<ConvertedInput>, String> {
    let raw_inputs = extract_raw_inputs(&options.input_manifest)?;
    let mut conversions = Vec::new();
    for raw in &raw_inputs {
        match resolve_raw_input(raw, options) {
            Ok(records) => conversions.extend(
                records
                    .iter()
                    .map(|record| convert_resolved_input(record, options)),
            ),
            Err(reason) => conversions.push(failed_resolution(raw, &reason)),
        }
    }
    Ok(conversions)
}

fn extract_raw_inputs(path: &Path) -> Result<Vec<RawInput>, String> {
    let text = read_text(path)?;
    let value = serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}'))
        .map_err(|err| format!("invalid_input_manifest_json:{err}"))?;
    let mut raw = Vec::new();
    let mut file_count = 0usize;
    if let Some(inputs) = value.get("inputs").and_then(Value::as_array) {
        for item in inputs {
            let field = item.get("field").and_then(Value::as_str).unwrap_or("");
            let val = item
                .get("value")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim();
            if field != "project_name" && !val.is_empty() {
                raw.push(RawInput {
                    input_id: format!("input_{}", raw.len() + 1),
                    original_field: field.to_ascii_lowercase(),
                    value: val.to_string(),
                    file_count,
                });
            }
        }
    }
    if let Some(files) = value.get("files").and_then(Value::as_array) {
        file_count = files.len();
        for file in files {
            let field = file.get("field").and_then(Value::as_str).unwrap_or("");
            let name = file.get("name").and_then(Value::as_str).unwrap_or("");
            let persisted = file
                .get("persisted_path")
                .and_then(Value::as_str)
                .unwrap_or("");
            let key = format!("{field} {name}").to_ascii_lowercase();
            let val = if persisted.is_empty() {
                name
            } else {
                persisted
            };
            if field != "project_name" && !val.is_empty() {
                raw.push(RawInput {
                    input_id: format!("input_{}", raw.len() + 1),
                    original_field: key,
                    value: val.to_string(),
                    file_count,
                });
            }
        }
    }
    for item in &mut raw {
        item.file_count = file_count;
    }
    Ok(raw)
}

pub(super) fn normalized_inputs_from_conversions(
    conversions: &[ConvertedInput],
) -> Vec<NormalizedInput> {
    conversions
        .iter()
        .filter_map(|input| {
            let kind = input.canonical_kind?;
            let tokens = tokenize(&input.canonical_value);
            Some(NormalizedInput {
                input_id: input.input_id.clone(),
                record_id: input.record_id.clone(),
                kind,
                primary_value: input.canonical_value.clone(),
                file_count: input.file_count,
                tokens,
                source_kind: input.source_kind,
                source_path: input.source_path.clone(),
                content_sha256: input.content_sha256.clone(),
                structure_atom_count: input.structure_atom_count,
                structure_chain_count: input.structure_chain_count,
                structure_residue_count: input.structure_residue_count,
            })
        })
        .collect()
}

fn convert_resolved_input(
    record: &ResolvedInputRecord,
    options: &ScienceOptions,
) -> ConvertedInput {
    if is_molecule_format(&record.format) {
        return converted_input(
            record,
            ConversionDetails {
                kind: InputKind::SmallMolecule,
                canonical_value: record.content.clone(),
                inferred_kind: "small_molecule",
                method: "rdkit_structure_file",
                format: &record.format,
                artifact: "",
                risk_flag: "",
            },
        );
    }
    if is_coordinate_format(&record.format) {
        let kind = coordinate_kind(&record.original_field);
        return converted_input(
            record,
            ConversionDetails {
                kind,
                canonical_value: record.content.clone(),
                inferred_kind: kind.as_str(),
                method: "declared_structure_file",
                format: &record.format,
                artifact: "",
                risk_flag: "",
            },
        );
    }
    match normalized_field(&record.original_field) {
        "small_molecule" | "smiles" | "inchi" | "cas" => convert_small_molecule(record, options),
        "peptide" => convert_amino_acid_sequence(record, InputKind::Peptide),
        "protein_ligand" => convert_amino_acid_sequence(record, InputKind::ProteinLigand),
        "genome_or_metagenome" => convert_nucleotide_sequence(record),
        "complex" => convert_complex(record),
        _ => infer_untyped_record(record, options),
    }
}

fn normalized_field(field: &str) -> &'static str {
    match field.split_whitespace().next().unwrap_or("") {
        "small_molecule" | "small-molecule" | "molecule" | "ligand" => "small_molecule",
        "smiles" => "smiles",
        "inchi" => "inchi",
        "cas" => "cas",
        "peptide" => "peptide",
        "protein_ligand" | "protein" => "protein_ligand",
        "genome_or_metagenome" | "genome" | "metagenome" | "nucleotide" => "genome_or_metagenome",
        "complex" => "complex",
        _ => "untyped",
    }
}

fn convert_small_molecule(
    record: &ResolvedInputRecord,
    options: &ScienceOptions,
) -> ConvertedInput {
    let value = record.content.trim();
    let field = normalized_field(&record.original_field);
    if field == "inchi" || looks_like_inchi(value) {
        return match canonicalize_molecular_text(value, "inchi") {
            Ok(canonical_value) => converted_molecular_text(record, canonical_value, "rdkit_inchi"),
            Err(error) => failed_conversion_with_reason(
                record,
                &format!("invalid_inchi:{error}"),
                "molecular_structure_parse_failed",
            ),
        };
    }
    if field == "smiles" {
        return match canonicalize_molecular_text(value, "smiles") {
            Ok(canonical_value) => {
                converted_molecular_text(record, canonical_value, "rdkit_smiles")
            }
            Err(error) => failed_conversion_with_reason(
                record,
                &format!("invalid_smiles:{error}"),
                "molecular_structure_parse_failed",
            ),
        };
    }
    if field != "cas"
        && let Ok(canonical_value) = canonicalize_molecular_text(value, "smiles")
    {
        return converted_molecular_text(record, canonical_value, "rdkit_smiles");
    }

    let (identifier_type, identifier) = if field == "cas" || looks_like_cas_number(value) {
        match normalize_cas_identifier(value) {
            Ok(identifier) => ("cas", identifier),
            Err(reason) => {
                return failed_conversion_with_reason(record, reason, "invalid_cas_identifier");
            }
        }
    } else {
        ("name", normalize_compound_name(value))
    };
    let mapped_smiles = match lookup_compound_identifier(options, identifier_type, &identifier) {
        Ok(Some(smiles)) => smiles,
        Ok(None) => {
            return failed_conversion_with_reason(
                record,
                "unknown_compound_identifier",
                "no_local_name_or_cas_mapping",
            );
        }
        Err(reason) => {
            return failed_conversion_with_reason(
                record,
                &reason,
                "compound_identifier_index_invalid",
            );
        }
    };
    match canonicalize_molecular_text(&mapped_smiles, "smiles") {
        Ok(canonical_value) => {
            converted_molecular_text(record, canonical_value, "local_identifier_lookup")
        }
        Err(error) => failed_conversion_with_reason(
            record,
            &format!("invalid_mapped_smiles:{error}"),
            "molecular_structure_parse_failed",
        ),
    }
}

fn converted_molecular_text(
    record: &ResolvedInputRecord,
    canonical_value: String,
    method: &'static str,
) -> ConvertedInput {
    converted_input(
        record,
        ConversionDetails {
            kind: InputKind::SmallMolecule,
            canonical_value,
            inferred_kind: "small_molecule",
            method,
            format: "plain_text",
            artifact: "",
            risk_flag: "",
        },
    )
}

fn convert_amino_acid_sequence(record: &ResolvedInputRecord, kind: InputKind) -> ConvertedInput {
    let sequence = compact_sequence(&record.content).to_ascii_uppercase();
    if sequence.len() < 2 || !sequence.chars().all(is_standard_amino_acid) {
        return failed_conversion_with_reason(
            record,
            if kind == InputKind::Peptide {
                "invalid_peptide_sequence"
            } else {
                "invalid_protein_sequence"
            },
            "invalid_declared_sequence",
        );
    }
    converted_input(
        record,
        ConversionDetails {
            kind,
            canonical_value: sequence,
            inferred_kind: kind.as_str(),
            method: "standard_amino_acid_sequence",
            format: sequence_text_format(&record.content),
            artifact: "",
            risk_flag: "",
        },
    )
}

fn convert_nucleotide_sequence(record: &ResolvedInputRecord) -> ConvertedInput {
    let sequence = compact_sequence(&record.content).to_ascii_uppercase();
    if sequence.len() < 8 || !sequence.chars().all(is_nucleotide_symbol) {
        return failed_conversion_with_reason(
            record,
            "invalid_nucleotide_sequence",
            "invalid_declared_sequence",
        );
    }
    converted_input(
        record,
        ConversionDetails {
            kind: InputKind::GenomeOrMetagenome,
            canonical_value: sequence,
            inferred_kind: "genome_or_metagenome",
            method: "nucleotide_sequence",
            format: sequence_text_format(&record.content),
            artifact: "",
            risk_flag: "",
        },
    )
}

fn convert_complex(record: &ResolvedInputRecord) -> ConvertedInput {
    let value = record.content.trim();
    if count_alphanumeric(value) < 5 {
        return failed_conversion_with_reason(
            record,
            "invalid_complex_description",
            "requires_structure_or_complex_context",
        );
    }
    converted_input(
        record,
        ConversionDetails {
            kind: InputKind::Complex,
            canonical_value: value.to_string(),
            inferred_kind: "complex",
            method: "complex_description",
            format: "plain_text",
            artifact: "",
            risk_flag: "requires_structure_or_complex_context",
        },
    )
}

fn infer_untyped_record(record: &ResolvedInputRecord, options: &ScienceOptions) -> ConvertedInput {
    let sequence = compact_sequence(&record.content).to_ascii_uppercase();
    if sequence.len() >= 8 && sequence.chars().all(is_nucleotide_symbol) {
        return convert_nucleotide_sequence(record);
    }
    if sequence.len() >= 2 && sequence.chars().all(is_standard_amino_acid) {
        let kind = if sequence.len() > 80 {
            InputKind::ProteinLigand
        } else {
            InputKind::Peptide
        };
        return convert_amino_acid_sequence(record, kind);
    }
    convert_small_molecule(record, options)
}

fn sequence_text_format(value: &str) -> &'static str {
    if value.trim_start().starts_with('>') {
        "fasta"
    } else {
        "plain_text"
    }
}

fn converted_input(record: &ResolvedInputRecord, details: ConversionDetails<'_>) -> ConvertedInput {
    ConvertedInput {
        input_id: record.input_id.clone(),
        record_id: record.record_id.clone(),
        original_field: record.original_field.clone(),
        inferred_kind: details.inferred_kind.to_string(),
        canonical_kind: Some(details.kind),
        canonical_value: details.canonical_value,
        conversion_status: "converted",
        conversion_method: details.method,
        format: if record.source_kind == "file" {
            record.format.clone()
        } else {
            details.format.to_string()
        },
        source_kind: record.source_kind,
        source_path: record.source_path.clone(),
        content_sha256: record.content_sha256.clone(),
        structure_atom_count: record.structure_atom_count,
        structure_chain_count: record.structure_chain_count,
        structure_residue_count: record.structure_residue_count,
        reason: "recognized as calculable input".to_string(),
        artifact: details.artifact.to_string(),
        risk_flags: if details.risk_flag.is_empty() {
            Vec::new()
        } else {
            vec![details.risk_flag.to_string()]
        },
        file_count: record.file_count,
    }
}

fn failed_conversion_with_reason(
    record: &ResolvedInputRecord,
    reason: &str,
    risk: &str,
) -> ConvertedInput {
    ConvertedInput {
        input_id: record.input_id.clone(),
        record_id: record.record_id.clone(),
        original_field: record.original_field.clone(),
        inferred_kind: "unknown".to_string(),
        canonical_kind: None,
        canonical_value: String::new(),
        conversion_status: "failed",
        conversion_method: "input_validation",
        format: record.format.clone(),
        source_kind: record.source_kind,
        source_path: record.source_path.clone(),
        content_sha256: record.content_sha256.clone(),
        structure_atom_count: record.structure_atom_count,
        structure_chain_count: record.structure_chain_count,
        structure_residue_count: record.structure_residue_count,
        reason: reason.to_string(),
        artifact: String::new(),
        risk_flags: if risk.is_empty() {
            Vec::new()
        } else {
            vec![risk.to_string()]
        },
        file_count: record.file_count,
    }
}

fn resolve_raw_input(
    raw: &RawInput,
    options: &ScienceOptions,
) -> Result<Vec<ResolvedInputRecord>, String> {
    let declared_value = raw.value.trim();
    let Some(format) = recognized_file_format(declared_value) else {
        return Ok(vec![ResolvedInputRecord {
            input_id: raw.input_id.clone(),
            record_id: "record_1".to_string(),
            original_field: raw.original_field.clone(),
            source_kind: "inline",
            source_path: String::new(),
            content_sha256: sha256_hex(declared_value.as_bytes()),
            format: "plain_text".to_string(),
            content: declared_value.to_string(),
            structure_atom_count: 0,
            structure_chain_count: 0,
            structure_residue_count: 0,
            file_count: raw.file_count,
        }]);
    };
    let source_path = resolve_input_path(declared_value, options);
    if !source_path.is_file() {
        return Err(format!("input_file_not_found:{declared_value}"));
    }
    if matches!(format, "gff" | "gtf" | "vcf") {
        return Err(format!(
            "input_file_requires_reference_sequence:{declared_value}"
        ));
    }
    let bytes = fs::read(&source_path)
        .map_err(|err| format!("input_file_read_failed:{declared_value}:{err}"))?;
    let content_sha256 = sha256_hex(&bytes);
    if is_molecule_format(format) {
        let records = resolve_molecule_file(&source_path, format, &content_sha256)?;
        return Ok(records
            .into_iter()
            .map(|(record_id, content)| {
                file_record(
                    raw,
                    record_id,
                    &source_path,
                    format,
                    &content_sha256,
                    content,
                )
            })
            .collect());
    }
    let text = String::from_utf8(bytes)
        .map_err(|err| format!("input_file_not_utf8:{declared_value}:{err}"))?;
    match format {
        "fasta" | "fa" | "faa" | "fna" => {
            resolve_fasta_records(raw, &source_path, format, &content_sha256, &text)
        }
        "fastq" | "fq" => resolve_fastq_records(raw, &source_path, format, &content_sha256, &text),
        "gb" | "gbk" => {
            let sequence = genbank_origin_sequence(&text)?;
            Ok(vec![file_record(
                raw,
                "record_1".to_string(),
                &source_path,
                format,
                &content_sha256,
                sequence,
            )])
        }
        "pdb" | "cif" | "mmcif" | "ent" => {
            let summary = resolve_coordinate_file(&source_path, format, &content_sha256)?;
            let mut record = file_record(
                raw,
                "record_1".to_string(),
                &source_path,
                format,
                &content_sha256,
                text,
            );
            record.structure_atom_count = summary.atom_count;
            record.structure_chain_count = summary.chain_count;
            record.structure_residue_count = summary.residue_count;
            Ok(vec![record])
        }
        _ => unreachable!("recognized file format must be handled"),
    }
}

fn recognized_file_format(value: &str) -> Option<&'static str> {
    let extension = Path::new(value.trim())
        .extension()
        .and_then(|ext| ext.to_str())?
        .to_ascii_lowercase();
    match extension.as_str() {
        "fasta" => Some("fasta"),
        "fa" => Some("fa"),
        "faa" => Some("faa"),
        "fna" => Some("fna"),
        "fastq" => Some("fastq"),
        "fq" => Some("fq"),
        "gb" => Some("gb"),
        "gbk" => Some("gbk"),
        "sdf" => Some("sdf"),
        "mol" => Some("mol"),
        "mol2" => Some("mol2"),
        "pdb" => Some("pdb"),
        "cif" => Some("cif"),
        "mmcif" => Some("mmcif"),
        "ent" => Some("ent"),
        "gff" => Some("gff"),
        "gtf" => Some("gtf"),
        "vcf" => Some("vcf"),
        _ => None,
    }
}

fn resolve_input_path(value: &str, options: &ScienceOptions) -> PathBuf {
    let declared = PathBuf::from(value);
    if declared.is_absolute() {
        return declared;
    }
    if let Some(parent) = options.input_manifest.parent() {
        let relative_to_manifest = parent.join(&declared);
        if relative_to_manifest.is_file() {
            return relative_to_manifest;
        }
    }
    let relative_to_workspace = options.workspace_root.join(&declared);
    if relative_to_workspace.is_file() {
        return relative_to_workspace;
    }
    relative_to_workspace
}

fn file_record(
    raw: &RawInput,
    record_id: String,
    source_path: &Path,
    format: &str,
    content_sha256: &str,
    content: String,
) -> ResolvedInputRecord {
    ResolvedInputRecord {
        input_id: raw.input_id.clone(),
        record_id,
        original_field: raw.original_field.clone(),
        source_kind: "file",
        source_path: source_path.display().to_string(),
        content_sha256: content_sha256.to_string(),
        format: format.to_string(),
        content,
        structure_atom_count: 0,
        structure_chain_count: 0,
        structure_residue_count: 0,
        file_count: raw.file_count,
    }
}

fn resolve_fasta_records(
    raw: &RawInput,
    source_path: &Path,
    format: &str,
    content_sha256: &str,
    text: &str,
) -> Result<Vec<ResolvedInputRecord>, String> {
    let mut records = Vec::new();
    let mut seen_record_ids = BTreeSet::new();
    let mut record_id = None::<String>;
    let mut sequence = String::new();
    for line in text.lines() {
        if let Some(header) = line.trim_start().strip_prefix('>') {
            let next_id = fasta_record_id(header, records.len() + 1);
            if !seen_record_ids.insert(next_id.clone()) {
                return Err(format!("duplicate_input_record_id:{next_id}"));
            }
            if let Some(previous_id) = record_id.replace(next_id) {
                if sequence.is_empty() {
                    return Err("invalid_fasta_record:empty_sequence".to_string());
                }
                records.push(file_record(
                    raw,
                    previous_id,
                    source_path,
                    format,
                    content_sha256,
                    std::mem::take(&mut sequence),
                ));
            }
        } else if record_id.is_some() {
            sequence.extend(line.chars().filter(|ch| !ch.is_whitespace() && *ch != '-'));
        } else if !line.trim().is_empty() {
            return Err("invalid_fasta_record:missing_header".to_string());
        }
    }
    let Some(last_id) = record_id else {
        return Err("invalid_fasta_record:missing_header".to_string());
    };
    if sequence.is_empty() {
        return Err("invalid_fasta_record:empty_sequence".to_string());
    }
    records.push(file_record(
        raw,
        last_id,
        source_path,
        format,
        content_sha256,
        sequence,
    ));
    Ok(records)
}

fn fasta_record_id(header: &str, index: usize) -> String {
    header
        .split_whitespace()
        .next()
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("record_{index}"))
}

fn resolve_fastq_records(
    raw: &RawInput,
    source_path: &Path,
    format: &str,
    content_sha256: &str,
    text: &str,
) -> Result<Vec<ResolvedInputRecord>, String> {
    let lines = text.lines().collect::<Vec<_>>();
    if lines.is_empty() || lines.len() % 4 != 0 {
        return Err("invalid_fastq_record:line_count".to_string());
    }
    let mut records = Vec::new();
    let mut seen_record_ids = BTreeSet::new();
    for chunk in lines.chunks_exact(4) {
        let Some(header) = chunk[0].strip_prefix('@') else {
            return Err("invalid_fastq_record:missing_header".to_string());
        };
        if !chunk[2].starts_with('+') {
            return Err("invalid_fastq_record:missing_separator".to_string());
        }
        let sequence = chunk[1].trim();
        let quality = chunk[3].trim();
        if sequence.chars().count() != quality.chars().count() {
            return Err("invalid_fastq_record:sequence_quality_length".to_string());
        }
        let record_id = fasta_record_id(header, records.len() + 1);
        if !seen_record_ids.insert(record_id.clone()) {
            return Err(format!("duplicate_input_record_id:{record_id}"));
        }
        records.push(file_record(
            raw,
            record_id,
            source_path,
            format,
            content_sha256,
            sequence.to_string(),
        ));
    }
    Ok(records)
}

fn genbank_origin_sequence(text: &str) -> Result<String, String> {
    let mut in_origin = false;
    let mut sequence = String::new();
    for line in text.lines() {
        if !in_origin {
            if line.trim().eq_ignore_ascii_case("ORIGIN") {
                in_origin = true;
            }
            continue;
        }
        if line.trim_start().starts_with("//") {
            break;
        }
        sequence.extend(
            line.chars()
                .filter(|ch| !ch.is_ascii_digit() && !ch.is_whitespace()),
        );
    }
    if !in_origin || sequence.is_empty() {
        return Err("invalid_genbank_origin".to_string());
    }
    Ok(sequence.to_ascii_uppercase())
}

fn resolve_molecule_file(
    path: &Path,
    format: &str,
    content_sha256: &str,
) -> Result<Vec<(String, String)>, String> {
    let args = vec![
        "--path".to_string(),
        path.display().to_string(),
        "--format".to_string(),
        format.to_string(),
    ];
    let context = format!("input_resolver_{}", &content_sha256[..12]);
    let output = run_python_script(INPUT_RESOLVER_SCRIPT, &args, &context)?;
    let value = serde_json::from_str::<Value>(output.trim())
        .map_err(|err| format!("input_resolver_invalid_json:{err}"))?;
    let records = value
        .get("records")
        .and_then(Value::as_array)
        .ok_or_else(|| "input_resolver_missing_records".to_string())?;
    if records.is_empty() {
        return Err("input_resolver_empty_records".to_string());
    }
    records
        .iter()
        .enumerate()
        .map(|(index, record)| {
            let record_id = record
                .get("record_id")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| format!("record_{}", index + 1));
            let canonical_smiles = record
                .get("canonical_smiles")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
                .ok_or_else(|| format!("input_resolver_missing_canonical_smiles:{record_id}"))?;
            Ok((record_id, canonical_smiles))
        })
        .collect()
}

fn canonicalize_molecular_text(value: &str, format: &str) -> Result<String, String> {
    let args = vec![
        "--text-hex".to_string(),
        value
            .as_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
        "--format".to_string(),
        format.to_string(),
    ];
    let digest = sha256_hex(value.as_bytes());
    let context = format!("input_resolver_{}", &digest[..12]);
    let output = run_python_script(INPUT_RESOLVER_SCRIPT, &args, &context)?;
    let value = serde_json::from_str::<Value>(output.trim())
        .map_err(|err| format!("input_resolver_invalid_json:{err}"))?;
    let records = value
        .get("records")
        .and_then(Value::as_array)
        .ok_or_else(|| "input_resolver_missing_records".to_string())?;
    if records.len() != 1 {
        return Err("input_resolver_invalid_inline_record_count".to_string());
    }
    records[0]
        .get("canonical_smiles")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| "input_resolver_missing_canonical_smiles:record_1".to_string())
}

fn lookup_compound_identifier(
    options: &ScienceOptions,
    identifier_type: &str,
    identifier: &str,
) -> Result<Option<String>, String> {
    let path = options
        .science_store
        .join("indexes/compound_identifiers.tsv");
    if !path.is_file() {
        return Ok(None);
    }
    let file = fs::File::open(&path)
        .map_err(|err| format!("compound_identifier_index_read_failed:{err}"))?;
    let mut lines = BufReader::new(file).lines();
    let header = lines
        .next()
        .transpose()
        .map_err(|err| format!("compound_identifier_index_read_failed:{err}"))?
        .unwrap_or_default();
    if header.trim_start_matches('\u{feff}')
        != "identifier_type\tidentifier\tcanonical_smiles\tdisplay_name\tsource"
    {
        return Err("compound_identifier_index_schema_mismatch".to_string());
    }
    for (index, line) in lines.enumerate() {
        let line = line.map_err(|err| format!("compound_identifier_index_read_failed:{err}"))?;
        if line.trim().is_empty() {
            continue;
        }
        let columns = line.split('\t').collect::<Vec<_>>();
        if columns.len() != 5 {
            return Err(format!(
                "compound_identifier_index_invalid_row:{}",
                index + 2
            ));
        }
        if !columns[0].eq_ignore_ascii_case(identifier_type) {
            continue;
        }
        let row_identifier = match identifier_type {
            "cas" => normalize_cas_identifier(columns[1])
                .map_err(|reason| format!("compound_identifier_index_{reason}:{}", index + 2))?,
            "name" => normalize_compound_name(columns[1]),
            _ => continue,
        };
        if row_identifier == identifier {
            return Ok(Some(columns[2].to_string()));
        }
    }
    Ok(None)
}

fn resolve_coordinate_file(
    path: &Path,
    format: &str,
    content_sha256: &str,
) -> Result<CoordinateSummary, String> {
    let args = vec![
        "--path".to_string(),
        path.display().to_string(),
        "--format".to_string(),
        format.to_string(),
    ];
    let context = format!("input_resolver_{}", &content_sha256[..12]);
    let output = run_python_script(INPUT_RESOLVER_SCRIPT, &args, &context)?;
    let value = serde_json::from_str::<Value>(output.trim())
        .map_err(|err| format!("input_resolver_invalid_json:{err}"))?;
    let structure = value
        .get("structure")
        .ok_or_else(|| "input_resolver_missing_structure".to_string())?;
    let parse_count = |name: &str| {
        structure
            .get(name)
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .filter(|value| *value > 0)
            .ok_or_else(|| format!("input_resolver_invalid_structure_{name}"))
    };
    Ok(CoordinateSummary {
        atom_count: parse_count("atom_count")?,
        chain_count: parse_count("chain_count")?,
        residue_count: parse_count("residue_count")?,
    })
}

fn failed_resolution(raw: &RawInput, reason: &str) -> ConvertedInput {
    let format = recognized_file_format(&raw.value).unwrap_or("file_path");
    ConvertedInput {
        input_id: raw.input_id.clone(),
        record_id: "record_1".to_string(),
        original_field: raw.original_field.clone(),
        inferred_kind: "unknown".to_string(),
        canonical_kind: None,
        canonical_value: String::new(),
        conversion_status: "failed",
        conversion_method: "file_resolution",
        format: format.to_string(),
        source_kind: "file",
        source_path: raw.value.clone(),
        content_sha256: String::new(),
        structure_atom_count: 0,
        structure_chain_count: 0,
        structure_residue_count: 0,
        reason: reason.to_string(),
        artifact: String::new(),
        risk_flags: vec!["input_file_resolution_failed".to_string()],
        file_count: raw.file_count,
    }
}

fn is_molecule_format(format: &str) -> bool {
    matches!(format, "sdf" | "mol" | "mol2")
}

fn is_coordinate_format(format: &str) -> bool {
    matches!(format, "pdb" | "cif" | "mmcif" | "ent")
}

fn coordinate_kind(field: &str) -> InputKind {
    if field.contains("peptide") || field.contains("protein") {
        InputKind::ProteinLigand
    } else if field.contains("small_molecule") || field.contains("ligand") {
        InputKind::SmallMolecule
    } else {
        InputKind::Complex
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn looks_like_inchi(value: &str) -> bool {
    value.trim_start().starts_with("InChI=")
}

fn looks_like_cas_number(value: &str) -> bool {
    let parts = value.trim().split('-').collect::<Vec<_>>();
    parts.len() == 3
        && (2..=7).contains(&parts[0].len())
        && parts[0].chars().all(|ch| ch.is_ascii_digit())
        && parts[1].len() == 2
        && parts[1].chars().all(|ch| ch.is_ascii_digit())
        && parts[2].len() == 1
        && parts[2].chars().all(|ch| ch.is_ascii_digit())
}

fn normalize_cas_identifier(value: &str) -> Result<String, &'static str> {
    let normalized = value.trim();
    if !looks_like_cas_number(normalized) {
        return Err("invalid_cas_format");
    }
    let digits = normalized
        .bytes()
        .filter(u8::is_ascii_digit)
        .map(|byte| usize::from(byte - b'0'))
        .collect::<Vec<_>>();
    let check_digit = *digits.last().ok_or("invalid_cas_format")?;
    let expected = digits[..digits.len() - 1]
        .iter()
        .rev()
        .enumerate()
        .map(|(index, digit)| (index + 1) * digit)
        .sum::<usize>()
        % 10;
    if check_digit != expected {
        return Err("invalid_cas_check_digit");
    }
    Ok(normalized.to_string())
}

fn normalize_compound_name(value: &str) -> String {
    value
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_nucleotide_symbol(ch: char) -> bool {
    matches!(
        ch.to_ascii_uppercase(),
        'A' | 'C'
            | 'G'
            | 'T'
            | 'U'
            | 'N'
            | 'R'
            | 'Y'
            | 'S'
            | 'W'
            | 'K'
            | 'M'
            | 'B'
            | 'D'
            | 'H'
            | 'V'
    )
}

fn is_standard_amino_acid(ch: char) -> bool {
    matches!(
        ch.to_ascii_uppercase(),
        'A' | 'C'
            | 'D'
            | 'E'
            | 'F'
            | 'G'
            | 'H'
            | 'I'
            | 'K'
            | 'L'
            | 'M'
            | 'N'
            | 'P'
            | 'Q'
            | 'R'
            | 'S'
            | 'T'
            | 'V'
            | 'W'
            | 'Y'
    )
}

fn compact_sequence(value: &str) -> String {
    let trimmed = value.trim();
    let body = if trimmed.starts_with('>') {
        trimmed
            .lines()
            .filter(|line| !line.trim_start().starts_with('>'))
            .collect::<Vec<_>>()
            .join("")
    } else {
        trimmed.to_string()
    };
    body.chars()
        .filter(|ch| !ch.is_whitespace() && *ch != '-')
        .collect()
}

fn count_alphanumeric(value: &str) -> usize {
    value.chars().filter(|ch| ch.is_alphanumeric()).count()
}

pub(super) fn write_input_conversion_manifest(
    options: &ScienceOptions,
    conversions: &[ConvertedInput],
) -> Result<(), String> {
    let mut text = format!("{INPUT_CONVERSION_HEADER}\n");
    for input in conversions {
        text.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            input.input_id,
            clean_cell(&input.record_id),
            clean_cell(&input.original_field),
            clean_cell(&input.inferred_kind),
            input
                .canonical_kind
                .map(InputKind::as_str)
                .unwrap_or("unknown"),
            encode_tsv_cell(&input.canonical_value),
            input.conversion_status,
            input.conversion_method,
            input.format,
            input.source_kind,
            clean_cell(&input.source_path),
            input.content_sha256,
            clean_cell(&input.reason),
            clean_cell(&input.artifact),
            clean_cell(&input.risk_flags.join(";")),
            input.structure_atom_count,
            input.structure_chain_count,
            input.structure_residue_count,
        ));
    }
    write_text(
        &options.run_dir.join("input_conversion_manifest.tsv"),
        &text,
    )?;
    write_json_manifest(
        &options.run_dir.join("input_conversion_manifest.json"),
        "input_conversion_manifest.tsv",
        "converted",
    )
}

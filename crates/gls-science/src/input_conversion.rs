use super::{InputKind, ScienceOptions, clean_cell, encode_tsv_cell, read_text, run_python_script};
#[cfg(test)]
use super::{NormalizedInput, tokenize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const INPUT_RESOLVER_SCRIPT: &str =
    include_str!("../../../python/genoligand_workers/input_resolver.py");
const INPUT_CAPABILITIES_JSON: &str = include_str!("../../../schemas/input_capabilities.json");
pub(super) const INPUT_MANIFEST_WRITE_BATCH_SIZE: usize = 256;
pub(super) const INPUT_CONVERSION_HEADER: &str = "input_id\trecord_id\toriginal_field\tinferred_kind\tcanonical_kind\tcanonical_value\tconversion_status\tconversion_method\tformat\tsource_kind\tsource_path\tcontent_sha256\treason\tartifact\trisk_flags\tstructure_atom_count\tstructure_chain_count\tstructure_residue_count";
static INPUT_CAPABILITIES: OnceLock<Value> = OnceLock::new();

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
    source_kind: &'static str,
    validation_error: Option<&'static str>,
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

fn input_capabilities() -> &'static Value {
    INPUT_CAPABILITIES.get_or_init(|| {
        serde_json::from_str(INPUT_CAPABILITIES_JSON)
            .expect("schemas/input_capabilities.json must contain valid JSON")
    })
}

fn input_capability(field: &str) -> Option<&'static Value> {
    let field = field.split_whitespace().next().unwrap_or("");
    input_capabilities()
        .get("input_types")?
        .as_array()?
        .iter()
        .find(|capability| {
            capability.get("field").and_then(Value::as_str) == Some(field)
                || capability
                    .get("aliases")
                    .and_then(Value::as_array)
                    .is_some_and(|aliases| {
                        aliases.iter().any(|alias| alias.as_str() == Some(field))
                    })
        })
}

fn input_file_extension_supported(extension: &str) -> bool {
    input_capabilities()
        .get("input_types")
        .and_then(Value::as_array)
        .is_some_and(|capabilities| {
            capabilities.iter().any(|capability| {
                capability
                    .pointer("/file/extensions")
                    .and_then(Value::as_array)
                    .is_some_and(|extensions| {
                        extensions
                            .iter()
                            .any(|candidate| candidate.as_str() == Some(extension))
                    })
            })
        })
}

#[cfg(test)]
pub(super) fn extract_inputs(options: &ScienceOptions) -> Result<Vec<NormalizedInput>, String> {
    Ok(normalized_inputs_from_conversions(
        &convert_inputs_from_manifest(options)?,
    ))
}

pub(super) fn input_manifest_has_declared_input(options: &ScienceOptions) -> Result<bool, String> {
    Ok(extract_raw_inputs(&options.input_manifest)?
        .iter()
        .any(|raw| raw.validation_error.is_none() && !raw.value.trim().is_empty()))
}

pub(super) fn convert_inputs_from_manifest(
    options: &ScienceOptions,
) -> Result<Vec<ConvertedInput>, String> {
    let mut conversions = Vec::new();
    for_each_converted_input(options, |conversion| {
        conversions.push(conversion.clone());
        Ok(())
    })?;
    Ok(conversions)
}

pub(super) fn for_each_converted_input<F>(
    options: &ScienceOptions,
    mut emit: F,
) -> Result<(), String>
where
    F: FnMut(&ConvertedInput) -> Result<(), String>,
{
    let raw_inputs = extract_raw_inputs(&options.input_manifest)?;
    for raw in &raw_inputs {
        if let Some(reason) = raw.validation_error {
            emit(&failed_manifest_input(raw, reason))?;
            continue;
        }
        if let Err(reason) = for_each_resolved_input(raw, options, |record| {
            emit(&convert_resolved_input(&record, options))
                .map_err(|err| format!("input_conversion_emit_failed:{err}"))
        }) {
            if let Some(err) = reason.strip_prefix("input_conversion_emit_failed:") {
                return Err(err.to_string());
            }
            emit(&failed_resolution(raw, &reason))?;
        }
    }
    Ok(())
}

fn extract_raw_inputs(path: &Path) -> Result<Vec<RawInput>, String> {
    let text = read_text(path)?;
    let value = serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}'))
        .map_err(|err| format!("invalid_input_manifest_json:{err}"))?;
    let mut raw = Vec::new();
    let mut input_index = 0usize;
    let mut file_count = 0usize;
    if let Some(input_section) = value.get("inputs") {
        let inputs = input_section
            .as_array()
            .ok_or_else(|| "invalid_input_manifest_schema:inputs_must_be_array".to_string())?;
        for item in inputs {
            let field = item.get("field").and_then(Value::as_str).unwrap_or("");
            if field == "project_name" {
                continue;
            }
            input_index += 1;
            let (value, validation_error) = manifest_string_value(item.get("value"));
            raw.push(RawInput {
                input_id: format!("input_{input_index}"),
                original_field: field.to_ascii_lowercase(),
                value,
                source_kind: "inline",
                validation_error,
                file_count,
            });
        }
    }
    if let Some(file_section) = value.get("files") {
        let files = file_section
            .as_array()
            .ok_or_else(|| "invalid_input_manifest_schema:files_must_be_array".to_string())?;
        file_count = files.len();
        for file in files {
            let field = file.get("field").and_then(Value::as_str).unwrap_or("");
            if field == "project_name" {
                continue;
            }
            input_index += 1;
            let name = file.get("name").and_then(Value::as_str).unwrap_or("");
            let key = format!("{field} {name}").to_ascii_lowercase();
            let (value, validation_error) = manifest_file_value(file);
            raw.push(RawInput {
                input_id: format!("input_{input_index}"),
                original_field: key,
                value,
                source_kind: "file",
                validation_error,
                file_count,
            });
        }
    }
    for item in &mut raw {
        item.file_count = file_count;
    }
    Ok(raw)
}

fn manifest_string_value(value: Option<&Value>) -> (String, Option<&'static str>) {
    match value {
        None => (String::new(), Some("missing_input_value")),
        Some(Value::String(value)) if value.trim().is_empty() => {
            (String::new(), Some("empty_input_value"))
        }
        Some(Value::String(value)) => (value.trim().to_string(), None),
        Some(_) => (String::new(), Some("invalid_input_value_type")),
    }
}

fn manifest_file_value(file: &Value) -> (String, Option<&'static str>) {
    match file.get("persisted_path") {
        Some(Value::String(value)) if !value.trim().is_empty() => (value.trim().to_string(), None),
        Some(Value::String(_)) | None => match file.get("name") {
            None if file.get("persisted_path").is_none() => {
                (String::new(), Some("missing_input_value"))
            }
            None => (String::new(), Some("empty_input_value")),
            Some(Value::String(value)) if value.trim().is_empty() => {
                (String::new(), Some("empty_input_value"))
            }
            Some(Value::String(value)) => (value.trim().to_string(), None),
            Some(_) => (String::new(), Some("invalid_input_value_type")),
        },
        Some(_) => (String::new(), Some("invalid_input_value_type")),
    }
}

#[cfg(test)]
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
    match normalized_field(&record.original_field) {
        "small_molecule" | "smiles" | "inchi" | "cas" => convert_small_molecule(record, options),
        "peptide" => convert_amino_acid_sequence(record, InputKind::Peptide),
        "protein_ligand" => convert_amino_acid_sequence(record, InputKind::ProteinLigand),
        "genome_or_metagenome" => convert_nucleotide_or_protein_sequence(record),
        "complex" => convert_complex(record),
        _ => infer_untyped_record(record, options),
    }
}

fn normalized_field(field: &str) -> &'static str {
    let field = field.split_whitespace().next().unwrap_or("");
    if matches!(field, "smiles" | "inchi" | "cas") {
        return match field {
            "smiles" => "smiles",
            "inchi" => "inchi",
            "cas" => "cas",
            _ => unreachable!(),
        };
    }
    input_capability(field)
        .and_then(|capability| capability.get("kind"))
        .and_then(Value::as_str)
        .unwrap_or("untyped")
}

fn convert_small_molecule(
    record: &ResolvedInputRecord,
    options: &ScienceOptions,
) -> ConvertedInput {
    if is_molecule_format(&record.format) {
        return converted_molecule_file(record);
    }
    if record.source_kind == "file" {
        return failed_field_format(record, "small_molecule");
    }
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

fn converted_molecule_file(record: &ResolvedInputRecord) -> ConvertedInput {
    converted_input(
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
    )
}

fn converted_coordinate_file(record: &ResolvedInputRecord, kind: InputKind) -> ConvertedInput {
    converted_input(
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
    )
}

fn convert_amino_acid_sequence(record: &ResolvedInputRecord, kind: InputKind) -> ConvertedInput {
    if is_coordinate_format(&record.format) {
        return converted_coordinate_file(record, kind);
    }
    if is_molecule_format(&record.format) {
        return failed_field_format(record, kind.as_str());
    }
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

fn convert_nucleotide_or_protein_sequence(record: &ResolvedInputRecord) -> ConvertedInput {
    if is_molecule_format(&record.format) || is_coordinate_format(&record.format) {
        return failed_field_format(record, "genome_or_metagenome");
    }
    let sequence = compact_sequence(&record.content).to_ascii_uppercase();
    if sequence.len() >= 8 && sequence.chars().all(is_nucleotide_symbol) {
        return converted_input(
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
        );
    }
    if sequence.len() >= 2 && sequence.chars().all(is_standard_amino_acid) {
        return converted_input(
            record,
            ConversionDetails {
                kind: InputKind::GenomeOrMetagenome,
                canonical_value: sequence,
                inferred_kind: "genome_or_metagenome",
                method: "standard_amino_acid_sequence",
                format: sequence_text_format(&record.content),
                artifact: "",
                risk_flag: "",
            },
        );
    }
    failed_conversion_with_reason(
        record,
        "invalid_nucleotide_or_protein_sequence",
        "invalid_declared_sequence",
    )
}

fn convert_complex(record: &ResolvedInputRecord) -> ConvertedInput {
    if is_coordinate_format(&record.format) {
        return converted_coordinate_file(record, InputKind::Complex);
    }
    if record.source_kind == "file" {
        return failed_field_format(record, "complex");
    }
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
    if is_molecule_format(&record.format) {
        return converted_molecule_file(record);
    }
    if is_coordinate_format(&record.format) {
        return converted_coordinate_file(record, coordinate_kind(&record.original_field));
    }
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

fn failed_field_format(record: &ResolvedInputRecord, field: &str) -> ConvertedInput {
    failed_conversion_with_reason(
        record,
        &format!("field_format_incompatible:{field}:{}", record.format),
        "declared_field_format_incompatible",
    )
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

fn for_each_resolved_input<F>(
    raw: &RawInput,
    options: &ScienceOptions,
    mut emit: F,
) -> Result<(), String>
where
    F: FnMut(ResolvedInputRecord) -> Result<(), String>,
{
    let declared_value = raw.value.trim();
    let Some(format) = recognized_file_format(declared_value) else {
        if raw.source_kind == "file" {
            return Err("unsupported_input_file_format".to_string());
        }
        return emit(ResolvedInputRecord {
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
        });
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
    match format {
        "fasta" | "fa" | "faa" | "fna" => for_each_fasta_record(
            raw,
            &source_path,
            format,
            open_sequence_reader(&source_path, declared_value)?,
            emit,
        ),
        "fastq" | "fq" => for_each_fastq_record(
            raw,
            &source_path,
            format,
            open_sequence_reader(&source_path, declared_value)?,
            emit,
        ),
        "gb" | "gbk" => for_each_genbank_record(
            raw,
            &source_path,
            format,
            open_sequence_reader(&source_path, declared_value)?,
            emit,
        ),
        "sdf" | "mol" | "mol2" => {
            let content_sha256 = file_sha256_hex(&source_path, declared_value)?;
            let records = resolve_molecule_file(&source_path, format, &content_sha256)?;
            for (record_id, content) in records {
                emit(file_record(
                    raw,
                    record_id,
                    &source_path,
                    format,
                    &content_sha256,
                    content,
                ))?;
            }
            Ok(())
        }
        "pdb" | "cif" | "mmcif" | "ent" => {
            let content_sha256 = file_sha256_hex(&source_path, declared_value)?;
            let summary = resolve_coordinate_file(&source_path, format, &content_sha256)?;
            let text = fs::read_to_string(&source_path)
                .map_err(|err| format!("input_file_not_utf8:{declared_value}:{err}"))?;
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
            emit(record)
        }
        _ => unreachable!("recognized file format must be handled"),
    }
}

fn open_sequence_reader(path: &Path, declared_value: &str) -> Result<BufReader<fs::File>, String> {
    fs::File::open(path)
        .map(BufReader::new)
        .map_err(|err| format!("input_file_read_failed:{declared_value}:{err}"))
}

fn file_sha256_hex(path: &Path, declared_value: &str) -> Result<String, String> {
    let mut reader = open_sequence_reader(path, declared_value)?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|err| format!("input_file_read_failed:{declared_value}:{err}"))?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    let digest = digest.finalize();
    Ok(format!("{digest:x}"))
}

fn recognized_file_format(value: &str) -> Option<&'static str> {
    let extension = Path::new(value.trim())
        .extension()
        .and_then(|ext| ext.to_str())?
        .to_ascii_lowercase();
    if !input_file_extension_supported(&extension) {
        return None;
    }
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

fn sequence_file_record(
    raw: &RawInput,
    record_id: String,
    source_path: &Path,
    format: &str,
    sequence: String,
) -> ResolvedInputRecord {
    let sequence = sequence.to_ascii_uppercase();
    let content_sha256 = sha256_hex(sequence.as_bytes());
    file_record(
        raw,
        record_id,
        source_path,
        format,
        &content_sha256,
        sequence,
    )
}

fn for_each_fasta_record<R: BufRead, F>(
    raw: &RawInput,
    source_path: &Path,
    format: &str,
    mut reader: R,
    mut emit: F,
) -> Result<(), String>
where
    F: FnMut(ResolvedInputRecord) -> Result<(), String>,
{
    let mut record_index = 0usize;
    let mut record_id = None::<String>;
    let mut sequence = String::new();
    let mut buffer = String::new();
    let mut line_number = 0usize;
    loop {
        buffer.clear();
        let bytes = reader.read_line(&mut buffer).map_err(|err| {
            format!(
                "input_file_not_utf8:{}:line_{}:{err}",
                source_path.display(),
                line_number + 1
            )
        })?;
        if bytes == 0 {
            break;
        }
        line_number += 1;
        let line = buffer.trim_end_matches(['\r', '\n']);
        if let Some(header) = line.trim_start().strip_prefix('>') {
            record_index += 1;
            let next_id = fasta_record_id(header, record_index);
            if let Some(previous_id) = record_id.replace(next_id) {
                if sequence.is_empty() {
                    return Err(format!(
                        "invalid_fasta_record:empty_sequence:line_{line_number}"
                    ));
                }
                emit(sequence_file_record(
                    raw,
                    previous_id,
                    source_path,
                    format,
                    std::mem::take(&mut sequence),
                ))?;
            }
        } else if record_id.is_some() {
            sequence.extend(line.chars().filter(|ch| !ch.is_whitespace() && *ch != '-'));
        } else if !line.trim().is_empty() {
            return Err(format!(
                "invalid_fasta_record:missing_header:line_{line_number}"
            ));
        }
    }
    let Some(last_id) = record_id else {
        return Err("invalid_fasta_record:missing_header:line_1".to_string());
    };
    if sequence.is_empty() {
        return Err(format!(
            "invalid_fasta_record:empty_sequence:line_{}",
            line_number.max(1)
        ));
    }
    emit(sequence_file_record(
        raw,
        last_id,
        source_path,
        format,
        sequence,
    ))?;
    Ok(())
}

fn fasta_record_id(header: &str, index: usize) -> String {
    let source_id = header
        .split_whitespace()
        .next()
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| "record".to_string());
    format!("{source_id}__record_{index}")
}

fn read_sequence_line<R: BufRead>(
    reader: &mut R,
    source_path: &Path,
    line_number: usize,
) -> Result<Option<String>, String> {
    let mut line = String::new();
    let bytes = reader.read_line(&mut line).map_err(|err| {
        format!(
            "input_file_not_utf8:{}:line_{line_number}:{err}",
            source_path.display()
        )
    })?;
    if bytes == 0 {
        return Ok(None);
    }
    while matches!(line.chars().last(), Some('\r' | '\n')) {
        line.pop();
    }
    Ok(Some(line))
}

fn for_each_fastq_record<R: BufRead, F>(
    raw: &RawInput,
    source_path: &Path,
    format: &str,
    mut reader: R,
    mut emit: F,
) -> Result<(), String>
where
    F: FnMut(ResolvedInputRecord) -> Result<(), String>,
{
    let mut emitted = 0usize;
    let mut line_number = 1usize;
    while let Some(header_line) = read_sequence_line(&mut reader, source_path, line_number)? {
        let Some(sequence_line) = read_sequence_line(&mut reader, source_path, line_number + 1)?
        else {
            return Err(format!(
                "invalid_fastq_record:line_count:line_{}",
                line_number + 1
            ));
        };
        let Some(separator_line) = read_sequence_line(&mut reader, source_path, line_number + 2)?
        else {
            return Err(format!(
                "invalid_fastq_record:line_count:line_{}",
                line_number + 2
            ));
        };
        let Some(quality_line) = read_sequence_line(&mut reader, source_path, line_number + 3)?
        else {
            return Err(format!(
                "invalid_fastq_record:line_count:line_{}",
                line_number + 3
            ));
        };
        let Some(header) = header_line.strip_prefix('@') else {
            return Err(format!(
                "invalid_fastq_record:missing_header:line_{line_number}"
            ));
        };
        if !separator_line.starts_with('+') {
            return Err(format!(
                "invalid_fastq_record:missing_separator:line_{}",
                line_number + 2
            ));
        }
        let sequence = sequence_line.trim();
        let quality = quality_line.trim();
        if sequence.is_empty() {
            return Err(format!(
                "invalid_fastq_record:empty_sequence:line_{}",
                line_number + 1
            ));
        }
        if sequence.chars().count() != quality.chars().count() {
            return Err(format!(
                "invalid_fastq_record:sequence_quality_length:line_{}",
                line_number + 3
            ));
        }
        let record_id = fasta_record_id(header, emitted + 1);
        emit(sequence_file_record(
            raw,
            record_id,
            source_path,
            format,
            sequence.to_string(),
        ))?;
        emitted += 1;
        line_number += 4;
    }
    if emitted == 0 {
        return Err("invalid_fastq_record:line_count:line_1".to_string());
    }
    Ok(())
}

fn for_each_genbank_record<R: BufRead, F>(
    raw: &RawInput,
    source_path: &Path,
    format: &str,
    mut reader: R,
    mut emit: F,
) -> Result<(), String>
where
    F: FnMut(ResolvedInputRecord) -> Result<(), String>,
{
    let mut emitted = 0usize;
    let mut locus = None::<String>;
    let mut record_id = None::<String>;
    let mut in_origin = false;
    let mut sequence = String::new();
    let mut line_number = 0usize;
    while let Some(line) = read_sequence_line(&mut reader, source_path, line_number + 1)? {
        line_number += 1;
        if !in_origin {
            if let Some(value) = line.strip_prefix("LOCUS") {
                locus = value
                    .split_whitespace()
                    .next()
                    .filter(|value| !value.is_empty())
                    .map(ToOwned::to_owned);
            }
            if line.trim().eq_ignore_ascii_case("ORIGIN") {
                in_origin = true;
                let source_id = locus.take().unwrap_or_else(|| "record".to_string());
                record_id = Some(format!("{source_id}__record_{}", emitted + 1));
            }
            continue;
        }
        if line.trim_start().starts_with("//") {
            let id = record_id
                .take()
                .ok_or_else(|| format!("invalid_genbank_origin:line_{line_number}"))?;
            if sequence.is_empty() {
                return Err(format!(
                    "invalid_genbank_origin:empty_sequence:line_{line_number}"
                ));
            }
            emit(sequence_file_record(
                raw,
                id,
                source_path,
                format,
                std::mem::take(&mut sequence),
            ))?;
            emitted += 1;
            in_origin = false;
            continue;
        }
        for ch in line.chars() {
            if ch.is_ascii_alphabetic() {
                sequence.push(ch);
            } else if !ch.is_ascii_digit() && !ch.is_whitespace() {
                return Err(format!(
                    "invalid_genbank_origin:invalid_symbol:line_{line_number}"
                ));
            }
        }
    }
    if in_origin {
        let id = record_id
            .take()
            .ok_or_else(|| format!("invalid_genbank_origin:line_{}", line_number.max(1)))?;
        if sequence.is_empty() {
            return Err(format!(
                "invalid_genbank_origin:empty_sequence:line_{}",
                line_number.max(1)
            ));
        }
        emit(sequence_file_record(raw, id, source_path, format, sequence))?;
        emitted += 1;
    }
    if emitted == 0 {
        return Err(format!(
            "invalid_genbank_origin:line_{}",
            line_number.max(1)
        ));
    }
    Ok(())
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

fn failed_manifest_input(raw: &RawInput, reason: &str) -> ConvertedInput {
    ConvertedInput {
        input_id: raw.input_id.clone(),
        record_id: "record_1".to_string(),
        original_field: raw.original_field.clone(),
        inferred_kind: "unknown".to_string(),
        canonical_kind: None,
        canonical_value: String::new(),
        conversion_status: "failed",
        conversion_method: "manifest_validation",
        format: if raw.source_kind == "file" {
            "file_path".to_string()
        } else {
            "plain_text".to_string()
        },
        source_kind: raw.source_kind,
        source_path: if raw.source_kind == "file" {
            raw.value.clone()
        } else {
            String::new()
        },
        content_sha256: String::new(),
        structure_atom_count: 0,
        structure_chain_count: 0,
        structure_residue_count: 0,
        reason: reason.to_string(),
        artifact: String::new(),
        risk_flags: vec!["manifest_input_invalid".to_string()],
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

pub(super) fn write_input_conversion_row<W: Write>(
    writer: &mut W,
    input: &ConvertedInput,
) -> std::io::Result<()> {
    writeln!(
        writer,
        "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
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
    )
}

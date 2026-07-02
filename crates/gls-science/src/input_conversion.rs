use super::{
    InputKind, NormalizedInput, ScienceOptions, clean_cell, encode_tsv_cell, read_text, tokenize,
    write_json_manifest, write_text,
};
use serde_json::Value;
use std::path::Path;

#[derive(Clone, Debug)]
pub(super) struct ConvertedInput {
    pub(super) input_id: String,
    pub(super) original_field: String,
    pub(super) inferred_kind: String,
    pub(super) canonical_kind: Option<InputKind>,
    pub(super) canonical_value: String,
    pub(super) conversion_status: &'static str,
    pub(super) conversion_method: &'static str,
    pub(super) format: &'static str,
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

struct ConversionDetails<'a> {
    kind: InputKind,
    canonical_value: String,
    inferred_kind: &'a str,
    method: &'static str,
    format: &'static str,
    artifact: &'a str,
    risk_flag: &'a str,
}

pub(super) fn extract_inputs(path: &Path) -> Result<Vec<NormalizedInput>, String> {
    Ok(normalized_inputs_from_conversions(
        &convert_inputs_from_manifest(path)?,
    ))
}

pub(super) fn convert_inputs_from_manifest(path: &Path) -> Result<Vec<ConvertedInput>, String> {
    let raw_inputs = extract_raw_inputs(path)?;
    Ok(raw_inputs.iter().map(convert_raw_input).collect::<Vec<_>>())
}

fn extract_raw_inputs(path: &Path) -> Result<Vec<RawInput>, String> {
    let text = read_text(path)?;
    let value =
        serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')).unwrap_or(Value::Null);
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
                kind,
                primary_value: input.canonical_value.clone(),
                file_count: input.file_count,
                tokens,
            })
        })
        .collect()
}

fn convert_raw_input(raw: &RawInput) -> ConvertedInput {
    let value = raw.value.trim();
    if let Some((kind, method, format)) = classify_path_input(&raw.original_field, value) {
        return converted_input(
            raw,
            ConversionDetails {
                kind,
                canonical_value: value.to_string(),
                inferred_kind: kind.as_str(),
                method,
                format,
                artifact: "",
                risk_flag: "",
            },
        );
    }
    let sequence = compact_sequence(value).to_ascii_uppercase();
    let sequence_format = if value.trim_start().starts_with('>') {
        "fasta"
    } else {
        "plain_text"
    };
    if sequence.len() >= 8
        && !raw.original_field.contains("peptide")
        && !raw.original_field.contains("protein")
        && sequence.chars().all(is_nucleotide_symbol)
    {
        return converted_input(
            raw,
            ConversionDetails {
                kind: InputKind::GenomeOrMetagenome,
                canonical_value: sequence,
                inferred_kind: "genome_or_metagenome",
                method: "nucleotide_sequence",
                format: sequence_format,
                artifact: "",
                risk_flag: "",
            },
        );
    }
    if raw.original_field.contains("small_molecule") && looks_like_smiles_text(value) {
        return converted_input(
            raw,
            ConversionDetails {
                kind: InputKind::SmallMolecule,
                canonical_value: value.to_string(),
                inferred_kind: "small_molecule",
                method: "rdkit_smiles",
                format: "plain_text",
                artifact: "",
                risk_flag: "",
            },
        );
    }
    if sequence.len() >= 2 && sequence.chars().all(is_standard_amino_acid) {
        let kind = if sequence.len() > 80 {
            InputKind::ProteinLigand
        } else {
            InputKind::Peptide
        };
        return converted_input(
            raw,
            ConversionDetails {
                kind,
                canonical_value: sequence,
                inferred_kind: kind.as_str(),
                method: "standard_amino_acid_sequence",
                format: sequence_format,
                artifact: "",
                risk_flag: "",
            },
        );
    }
    if looks_like_inchi(value) {
        return converted_input(
            raw,
            ConversionDetails {
                kind: InputKind::SmallMolecule,
                canonical_value: value.to_string(),
                inferred_kind: "small_molecule",
                method: "inchi_text",
                format: "plain_text",
                artifact: "",
                risk_flag: "",
            },
        );
    }
    if looks_like_smiles_text(value) {
        return converted_input(
            raw,
            ConversionDetails {
                kind: InputKind::SmallMolecule,
                canonical_value: value.to_string(),
                inferred_kind: "small_molecule",
                method: "rdkit_smiles",
                format: "plain_text",
                artifact: "",
                risk_flag: "",
            },
        );
    }
    if raw.original_field.contains("complex") && count_alphanumeric(value) >= 5 {
        return converted_input(
            raw,
            ConversionDetails {
                kind: InputKind::Complex,
                canonical_value: value.to_string(),
                inferred_kind: "complex",
                method: "complex_description",
                format: "plain_text",
                artifact: "",
                risk_flag: "requires_structure_or_complex_context",
            },
        );
    }
    failed_conversion(raw, value)
}

fn converted_input(raw: &RawInput, details: ConversionDetails<'_>) -> ConvertedInput {
    ConvertedInput {
        input_id: raw.input_id.clone(),
        original_field: raw.original_field.clone(),
        inferred_kind: details.inferred_kind.to_string(),
        canonical_kind: Some(details.kind),
        canonical_value: details.canonical_value,
        conversion_status: "converted",
        conversion_method: details.method,
        format: details.format,
        reason: "recognized as calculable input".to_string(),
        artifact: details.artifact.to_string(),
        risk_flags: if details.risk_flag.is_empty() {
            Vec::new()
        } else {
            vec![details.risk_flag.to_string()]
        },
        file_count: raw.file_count,
    }
}

fn failed_conversion(raw: &RawInput, value: &str) -> ConvertedInput {
    let risk = if looks_like_cas_number(value) || raw.original_field.contains("cas") {
        "no_local_name_or_cas_mapping"
    } else {
        "unrecognized_free_text"
    };
    ConvertedInput {
        input_id: raw.input_id.clone(),
        original_field: raw.original_field.clone(),
        inferred_kind: "unknown".to_string(),
        canonical_kind: None,
        canonical_value: value.to_string(),
        conversion_status: "failed",
        conversion_method: "unrecognized_text",
        format: "plain_text",
        reason: "无法转换为可计算结构或序列".to_string(),
        artifact: String::new(),
        risk_flags: vec![risk.to_string()],
        file_count: raw.file_count,
    }
}

fn classify_path_input(
    field: &str,
    value: &str,
) -> Option<(InputKind, &'static str, &'static str)> {
    let lower = value.trim().to_ascii_lowercase();
    let extension = Path::new(lower.trim())
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("");
    match extension {
        "sdf" | "mol" | "mol2" => Some((
            InputKind::SmallMolecule,
            "declared_structure_file",
            "file_path",
        )),
        "fasta" | "fa" | "faa" => {
            if field.contains("genome") {
                Some((
                    InputKind::GenomeOrMetagenome,
                    "declared_sequence_file",
                    "file_path",
                ))
            } else {
                Some((
                    InputKind::ProteinLigand,
                    "declared_sequence_file",
                    "file_path",
                ))
            }
        }
        "fna" | "fastq" | "fq" | "gb" | "gff" | "gtf" | "vcf" => Some((
            InputKind::GenomeOrMetagenome,
            "declared_sequence_file",
            "file_path",
        )),
        "pdb" | "cif" | "mmcif" | "ent" => {
            if field.contains("peptide") || field.contains("protein") {
                Some((
                    InputKind::ProteinLigand,
                    "declared_structure_file",
                    "file_path",
                ))
            } else if field.contains("small_molecule") || field.contains("ligand") {
                Some((
                    InputKind::SmallMolecule,
                    "declared_structure_file",
                    "file_path",
                ))
            } else {
                Some((InputKind::Complex, "declared_structure_file", "file_path"))
            }
        }
        _ => None,
    }
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

fn looks_like_smiles_text(value: &str) -> bool {
    let text = value.trim();
    if text.is_empty() || text.chars().any(char::is_whitespace) {
        return false;
    }
    if !text.chars().all(|ch| {
        ch.is_ascii_alphanumeric()
            || matches!(
                ch,
                '[' | ']' | '(' | ')' | '=' | '#' | '@' | '+' | '-' | '\\' | '/' | '%' | '.' | ':'
            )
    }) {
        return false;
    }
    let mut saw_atom = false;
    let chars = text.chars().collect::<Vec<_>>();
    let mut index = 0usize;
    while index < chars.len() {
        let ch = chars[index];
        if ch.is_ascii_uppercase() {
            let mut symbol = ch.to_string();
            if index + 1 < chars.len() && chars[index + 1].is_ascii_lowercase() {
                symbol.push(chars[index + 1]);
                if valid_element_symbol(&symbol) {
                    saw_atom = true;
                    index += 2;
                    continue;
                }
            }
            let single = ch.to_string();
            if valid_element_symbol(&single) {
                saw_atom = true;
                index += 1;
                continue;
            }
            return false;
        }
        if matches!(ch, 'b' | 'c' | 'n' | 'o' | 'p' | 's') {
            saw_atom = true;
            index += 1;
            continue;
        }
        if ch.is_ascii_lowercase() {
            return false;
        }
        index += 1;
    }
    saw_atom
}

fn valid_element_symbol(symbol: &str) -> bool {
    matches!(
        symbol,
        "B" | "C"
            | "N"
            | "O"
            | "P"
            | "S"
            | "F"
            | "I"
            | "H"
            | "Cl"
            | "Br"
            | "Na"
            | "K"
            | "Li"
            | "Mg"
            | "Ca"
            | "Zn"
            | "Fe"
    )
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
    let mut text = String::from(
        "input_id\toriginal_field\tinferred_kind\tcanonical_kind\tcanonical_value\tconversion_status\tconversion_method\tformat\treason\tartifact\trisk_flags\n",
    );
    for input in conversions {
        text.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            input.input_id,
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
            clean_cell(&input.reason),
            clean_cell(&input.artifact),
            clean_cell(&input.risk_flags.join(";"))
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

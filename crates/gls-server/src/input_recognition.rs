use serde_json::Value;

pub(super) fn input_value_has_usable_user_input(value: &Value) -> bool {
    let Some(inputs) = value.get("inputs").and_then(Value::as_array) else {
        return false;
    };
    let mut saw_usable_input = false;
    for item in inputs {
        let field = item.get("field").and_then(Value::as_str).unwrap_or("");
        let input_value = item.get("value").and_then(Value::as_str).unwrap_or("");
        if field == "project_name" || input_value.trim().is_empty() {
            continue;
        }
        if !input_field_is_usable(field, input_value) {
            return false;
        }
        saw_usable_input = true;
    }
    saw_usable_input
}

pub(super) fn input_recognition_summary(value: &Value) -> Option<String> {
    let inputs = value.get("inputs").and_then(Value::as_array)?;
    let mut labels = Vec::new();
    for item in inputs {
        let field = item.get("field").and_then(Value::as_str).unwrap_or("");
        let input_value = item.get("value").and_then(Value::as_str).unwrap_or("");
        if field == "project_name" || input_value.trim().is_empty() {
            continue;
        }
        let Some(label) = input_recognition_label(field, input_value) else {
            return Some("无法转换为可计算结构/序列".to_string());
        };
        if !labels.iter().any(|existing| existing == &label) {
            labels.push(label);
        }
    }
    if labels.is_empty() {
        None
    } else {
        Some(format!("已识别为{}", labels.join("、")))
    }
}

fn input_recognition_label(field: &str, value: &str) -> Option<String> {
    let field = field.to_ascii_lowercase();
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    if looks_like_path_with_ext(value, &["sdf", "mol", "mol2"]) {
        return Some("小分子结构文件".to_string());
    }
    if looks_like_path_with_ext(value, &["pdb", "cif", "mmcif", "ent"]) {
        return Some("结构文件".to_string());
    }
    if !field.contains("peptide")
        && !field.contains("protein")
        && looks_like_nucleotide_sequence(value, 8)
    {
        return Some("核酸序列".to_string());
    }
    if field.contains("small_molecule") && looks_like_smiles_text(value) {
        return Some("小分子".to_string());
    }
    if looks_like_peptide_sequence(value, 2) {
        let body_len = compact_sequence_body(value).len();
        return Some(if body_len > 80 {
            "蛋白序列".to_string()
        } else {
            "多肽".to_string()
        });
    }
    if looks_like_path_with_ext(value, &["fasta", "fa", "faa"]) {
        return Some("序列文件".to_string());
    }
    if looks_like_path_with_ext(value, &["fna", "fastq", "fq", "gb", "gff", "gtf", "vcf"]) {
        return Some("核酸序列文件".to_string());
    }
    if looks_like_inchi(value) || looks_like_smiles_text(value) {
        return Some("小分子".to_string());
    }
    if field == "complex" && count_alphanumeric(value) >= 5 {
        return Some("复合物描述".to_string());
    }
    None
}

fn input_field_is_usable(field: &str, value: &str) -> bool {
    let value = value.trim();
    if value.is_empty() || field == "project_name" {
        return false;
    }
    let field = field.to_ascii_lowercase();
    if looks_like_convertible_free_text(value) {
        return true;
    }
    match field.as_str() {
        "small_molecule" => false,
        "peptide" => {
            looks_like_path_with_ext(value, &["fasta", "fa", "pdb", "cif", "mmcif"])
                || looks_like_peptide_sequence(value, 2)
        }
        "protein_ligand" => {
            looks_like_path_with_ext(value, &["fasta", "fa", "faa", "pdb", "cif", "mmcif"])
                || looks_like_peptide_sequence(value, 5)
        }
        "complex" => {
            looks_like_path_with_ext(value, &["pdb", "cif", "mmcif", "ent"])
                || count_alphanumeric(value) >= 5
        }
        "genome_or_metagenome" => {
            looks_like_path_with_ext(
                value,
                &[
                    "fasta", "fa", "fna", "faa", "fastq", "fq", "gb", "gff", "gtf", "vcf",
                ],
            ) || looks_like_nucleotide_sequence(value, 8)
        }
        _ => count_alphanumeric(value) >= 2,
    }
}

fn looks_like_convertible_free_text(value: &str) -> bool {
    looks_like_path_with_ext(
        value,
        &[
            "sdf", "mol2", "mol", "pdb", "cif", "mmcif", "fasta", "fa", "faa", "fna", "fastq",
            "fq", "gb", "gff", "gtf", "vcf",
        ],
    ) || looks_like_peptide_sequence(value, 2)
        || looks_like_inchi(value)
        || looks_like_smiles_text(value)
        || looks_like_nucleotide_sequence(value, 8)
}

fn looks_like_path_with_ext(value: &str, extensions: &[&str]) -> bool {
    let value = value.trim().to_ascii_lowercase();
    extensions
        .iter()
        .any(|ext| value.ends_with(&format!(".{ext}")) || value.contains(&format!(".{ext}\n")))
}

fn compact_sequence_body(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.starts_with('>') {
        return trimmed
            .lines()
            .filter(|line| !line.trim_start().starts_with('>'))
            .collect::<String>();
    }
    trimmed.chars().filter(|ch| !ch.is_whitespace()).collect()
}

fn looks_like_peptide_sequence(value: &str, min_len: usize) -> bool {
    let body = compact_sequence_body(value);
    body.len() >= min_len
        && body.chars().all(|ch| {
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
        })
}

fn looks_like_nucleotide_sequence(value: &str, min_len: usize) -> bool {
    let body = compact_sequence_body(value);
    body.len() >= min_len
        && body.chars().all(|ch| {
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
        })
}

fn looks_like_inchi(value: &str) -> bool {
    value.trim_start().starts_with("InChI=")
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
                return false;
            }
            if valid_element_symbol(&symbol) {
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

fn count_alphanumeric(value: &str) -> usize {
    value.chars().filter(|ch| ch.is_alphanumeric()).count()
}

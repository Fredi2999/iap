use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

use crate::LauncherError;

const DEFAULT_ALIGNMENT: u64 = 32;
const MAX_METADATA_ITEMS: u64 = 1_000_000;
const MAX_TENSORS: u64 = 10_000_000;
const MAX_NAME_BYTES: u64 = 1024 * 1024;

#[derive(Debug)]
struct TensorInfo {
    name: String,
    offset: u64,
}

/// Enthält Dateispannen in derselben Reihenfolge, in der `--gpu-layers` sie hinzufügt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GgufLayerLayout {
    pub block_layer_bytes: Vec<u64>,
    pub output_layer_bytes: u64,
    pub gpu_offload_order_bytes: Vec<u64>,
}

/// Liest nur GGUF-Metadaten, damit GPU-Budgets aus der konkreten Datei statt Schätzungen kommen.
pub fn inspect_gguf_layers(path: &Path) -> Result<GgufLayerLayout, LauncherError> {
    let mut file = File::open(path).map_err(|source| LauncherError::Io {
        action: "GGUF öffnen",
        path: path.to_path_buf(),
        source,
    })?;
    parse_gguf(&mut file, path)
}

fn parse_gguf(file: &mut File, path: &Path) -> Result<GgufLayerLayout, LauncherError> {
    let mut magic = [0_u8; 4];
    read_exact(file, &mut magic, path)?;
    if &magic != b"GGUF" {
        return invalid(path, "Datei beginnt nicht mit GGUF-Magic");
    }
    let version = read_u32(file, path)?;
    if !(2..=3).contains(&version) {
        return invalid(path, format!("nicht unterstützte GGUF-Version {version}"));
    }
    let tensor_count = read_u64(file, path)?;
    let metadata_count = read_u64(file, path)?;
    if tensor_count > MAX_TENSORS || metadata_count > MAX_METADATA_ITEMS {
        return invalid(path, "unplausible Anzahl von Metadaten oder Tensoren");
    }

    let mut alignment = DEFAULT_ALIGNMENT;
    for _ in 0..metadata_count {
        let key = read_string(file, path)?;
        let value_type = read_u32(file, path)?;
        if key == "general.alignment" && value_type == 4 {
            alignment = u64::from(read_u32(file, path)?);
        } else {
            skip_value(file, value_type, path)?;
        }
    }
    if alignment == 0 || !alignment.is_power_of_two() {
        return invalid(path, format!("ungültiges Alignment {alignment}"));
    }

    let tensor_capacity = usize::try_from(tensor_count)
        .map_err(|_| invalid_error(path, "Tensoranzahl passt nicht in den Adressraum"))?;
    let mut tensors = Vec::with_capacity(tensor_capacity);
    for _ in 0..tensor_count {
        let name = read_string(file, path)?;
        let dimensions = read_u32(file, path)?;
        if dimensions > 8 {
            return invalid(
                path,
                format!("Tensor `{name}` hat {dimensions} Dimensionen"),
            );
        }
        seek_forward(file, u64::from(dimensions) * 8, path)?;
        let _tensor_type = read_u32(file, path)?;
        let offset = read_u64(file, path)?;
        tensors.push(TensorInfo { name, offset });
    }
    let info_end = file.stream_position().map_err(|source| LauncherError::Io {
        action: "GGUF-Position lesen",
        path: path.to_path_buf(),
        source,
    })?;
    let data_start = align_up(info_end, alignment)
        .ok_or_else(|| invalid_error(path, "Datenoffset läuft über"))?;
    let file_bytes = file
        .metadata()
        .map_err(|source| LauncherError::Io {
            action: "GGUF-Dateigröße lesen",
            path: path.to_path_buf(),
            source,
        })?
        .len();
    if data_start > file_bytes {
        return invalid(path, "Tensor-Datenbereich liegt hinter dem Dateiende");
    }

    tensors.sort_by_key(|tensor| tensor.offset);
    let mut blocks = BTreeMap::<usize, u64>::new();
    let mut output_layer_bytes = 0_u64;
    for (index, tensor) in tensors.iter().enumerate() {
        let next_offset = tensors
            .get(index + 1)
            .map(|next| next.offset)
            .unwrap_or(file_bytes - data_start);
        if next_offset < tensor.offset || data_start.saturating_add(next_offset) > file_bytes {
            return invalid(
                path,
                format!("Tensor `{}` hat einen ungültigen Offset", tensor.name),
            );
        }
        let span = next_offset - tensor.offset;
        if let Some(layer) = block_number(&tensor.name) {
            let entry = blocks.entry(layer).or_default();
            *entry = entry.saturating_add(span);
        } else if tensor.name.starts_with("output.") {
            output_layer_bytes = output_layer_bytes.saturating_add(span);
        }
    }

    let Some(maximum_block) = blocks.keys().next_back().copied() else {
        return invalid(path, "keine `blk.N.*`-Tensoren gefunden");
    };
    let mut block_layer_bytes = Vec::with_capacity(maximum_block + 1);
    for block in 0..=maximum_block {
        let Some(bytes) = blocks.get(&block).copied() else {
            return invalid(path, format!("Block {block} fehlt"));
        };
        block_layer_bytes.push(bytes);
    }
    let mut gpu_offload_order_bytes = block_layer_bytes.iter().rev().copied().collect::<Vec<_>>();
    if output_layer_bytes > 0 {
        gpu_offload_order_bytes.push(output_layer_bytes);
    }
    Ok(GgufLayerLayout {
        block_layer_bytes,
        output_layer_bytes,
        gpu_offload_order_bytes,
    })
}

fn block_number(name: &str) -> Option<usize> {
    let rest = name.strip_prefix("blk.")?;
    let number = rest.split_once('.')?.0;
    number.parse().ok()
}

fn skip_value(file: &mut File, value_type: u32, path: &Path) -> Result<(), LauncherError> {
    match value_type {
        0 | 1 | 7 => seek_forward(file, 1, path),
        2 | 3 => seek_forward(file, 2, path),
        4..=6 => seek_forward(file, 4, path),
        8 => skip_string(file, path),
        9 => {
            let element_type = read_u32(file, path)?;
            if element_type == 9 {
                return invalid(path, "verschachtelte GGUF-Arrays werden nicht unterstützt");
            }
            let count = read_u64(file, path)?;
            if count > u64::from(u32::MAX) {
                return invalid(path, "unplausibel großes GGUF-Array");
            }
            if let Some(size) = primitive_size(element_type) {
                let bytes = count
                    .checked_mul(size)
                    .ok_or_else(|| invalid_error(path, "GGUF-Arraygröße läuft über"))?;
                seek_forward(file, bytes, path)
            } else if element_type == 8 {
                for _ in 0..count {
                    skip_string(file, path)?;
                }
                Ok(())
            } else {
                invalid(path, format!("unbekannter GGUF-Arraytyp {element_type}"))
            }
        }
        10..=12 => seek_forward(file, 8, path),
        other => invalid(path, format!("unbekannter GGUF-Werttyp {other}")),
    }
}

fn primitive_size(value_type: u32) -> Option<u64> {
    match value_type {
        0 | 1 | 7 => Some(1),
        2 | 3 => Some(2),
        4..=6 => Some(4),
        10..=12 => Some(8),
        _ => None,
    }
}

fn read_string(file: &mut File, path: &Path) -> Result<String, LauncherError> {
    let length = read_u64(file, path)?;
    if length > MAX_NAME_BYTES {
        return invalid(path, format!("Name mit {length} Bytes ist unplausibel"));
    }
    let length = usize::try_from(length)
        .map_err(|_| invalid_error(path, "Stringlänge passt nicht in den Adressraum"))?;
    let mut bytes = vec![0_u8; length];
    read_exact(file, &mut bytes, path)?;
    String::from_utf8(bytes)
        .map_err(|error| invalid_error(path, format!("Name ist nicht UTF-8: {error}")))
}

fn skip_string(file: &mut File, path: &Path) -> Result<(), LauncherError> {
    let length = read_u64(file, path)?;
    seek_forward(file, length, path)
}

fn read_u32(file: &mut File, path: &Path) -> Result<u32, LauncherError> {
    let mut bytes = [0_u8; 4];
    read_exact(file, &mut bytes, path)?;
    Ok(u32::from_le_bytes(bytes))
}

fn read_u64(file: &mut File, path: &Path) -> Result<u64, LauncherError> {
    let mut bytes = [0_u8; 8];
    read_exact(file, &mut bytes, path)?;
    Ok(u64::from_le_bytes(bytes))
}

fn read_exact(file: &mut File, buffer: &mut [u8], path: &Path) -> Result<(), LauncherError> {
    file.read_exact(buffer).map_err(|source| LauncherError::Io {
        action: "GGUF lesen",
        path: path.to_path_buf(),
        source,
    })
}

fn seek_forward(file: &mut File, bytes: u64, path: &Path) -> Result<(), LauncherError> {
    let offset =
        i64::try_from(bytes).map_err(|_| invalid_error(path, "GGUF-Sprung ist zu groß"))?;
    file.seek(SeekFrom::Current(offset))
        .map(|_| ())
        .map_err(|source| LauncherError::Io {
            action: "GGUF-Bereich überspringen",
            path: path.to_path_buf(),
            source,
        })
}

fn align_up(value: u64, alignment: u64) -> Option<u64> {
    value
        .checked_add(alignment.checked_sub(1)?)
        .map(|sum| sum & !(alignment - 1))
}

fn invalid<T>(path: &Path, reason: impl Into<String>) -> Result<T, LauncherError> {
    Err(invalid_error(path, reason))
}

fn invalid_error(path: &Path, reason: impl Into<String>) -> LauncherError {
    LauncherError::InvalidGguf {
        path: PathBuf::from(path),
        reason: reason.into(),
    }
}

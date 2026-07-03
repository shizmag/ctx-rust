use ignore::DirEntry;

use ctx_models::NodeKind;

pub fn node_kind(entry: &DirEntry) -> NodeKind {
    let Some(file_type) = entry.file_type() else {
        return NodeKind::Other;
    };

    if file_type.is_file() {
        NodeKind::File
    } else if file_type.is_dir() {
        NodeKind::Directory
    } else if file_type.is_symlink() {
        NodeKind::Symlink
    } else {
        NodeKind::Other
    }
}

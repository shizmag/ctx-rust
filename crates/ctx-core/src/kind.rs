use ignore::DirEntry;

use ctx_models::NodeKind;

pub fn node_kind(entry: &DirEntry) -> NodeKind {
    let Some(file_type) = entry.file_type() else {
        return NodeKind::Other;
    };

    if file_type.is_file() {
        return NodeKind::File;
    } else if file_type.is_dir() {
        return NodeKind::Directory;
    } else if file_type.is_symlink() {
        return NodeKind::Symlink;
    } else {
        return NodeKind::Other;
    }
}

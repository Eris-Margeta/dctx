use crate::datatypes::{DirContextTreeNode, NodeType};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeType {
    Added,
    Removed,
    Modified,
}

#[derive(Debug, Clone)]
pub struct DiffEntry {
    pub change_type: ChangeType,
    pub node_type: NodeType,
    pub relative_path: String,
}

#[derive(Debug, Default, Clone)]
pub struct DiffStats {
    pub files_added: usize,
    pub files_removed: usize,
    pub files_modified: usize,
    /// Ključno za verzioniranje: koliko je fajlova ostalo identično.
    pub files_unchanged: usize, 
    /// Ukupan broj fajlova u novom stablu (korisno za statistiku).
    pub total_files_in_new_state: usize,
}

#[derive(Debug, Default)]
pub struct DiffReport {
    pub entries: Vec<DiffEntry>,
    pub stats: DiffStats,
    pub has_changes: bool,
}

/// Uspoređuje dva stabla i generira izvještaj.
/// `old_root` može biti None (u slučaju prvog run-a, sve je Added).
pub fn compare_trees(
    old_root: Option<&DirContextTreeNode>,
    new_root: &DirContextTreeNode,
) -> DiffReport {
    let mut report = DiffReport::default();

    match old_root {
        Some(old) => {
            compare_nodes_recursive(old, new_root, &mut report);
        }
        None => {
            // Ako nema starog stabla, sve u novom je ADDED
            mark_all_as_added_recursive(new_root, &mut report);
        }
    }

    report.has_changes = !report.entries.is_empty();
    
    // Izračunaj total_files_in_new_state (samo files, ne dirs)
    report.stats.total_files_in_new_state = count_files_recursive(new_root);

    report
}

// --- Helper Functions ---

fn count_files_recursive(node: &DirContextTreeNode) -> usize {
    let mut count = 0;
    if node.node_type == NodeType::File {
        count = 1;
    }
    if node.node_type == NodeType::Directory {
        for child in &node.children {
            count += count_files_recursive(child);
        }
    }
    count
}

fn mark_all_as_added_recursive(node: &DirContextTreeNode, report: &mut DiffReport) {
    // Dodajemo entry u diff listu
    report.entries.push(DiffEntry {
        change_type: ChangeType::Added,
        node_type: node.node_type,
        relative_path: node.relative_path.clone(),
    });

    // Ažuriramo statistiku
    if node.node_type == NodeType::File {
        report.stats.files_added += 1;
    }

    // Rekurzija za djecu
    if node.node_type == NodeType::Directory {
        for child in &node.children {
            mark_all_as_added_recursive(child, report);
        }
    }
}

fn mark_all_as_removed_recursive(node: &DirContextTreeNode, report: &mut DiffReport) {
    report.entries.push(DiffEntry {
        change_type: ChangeType::Removed,
        node_type: node.node_type,
        relative_path: node.relative_path.clone(),
    });

    if node.node_type == NodeType::File {
        report.stats.files_removed += 1;
    }

    if node.node_type == NodeType::Directory {
        for child in &node.children {
            mark_all_as_removed_recursive(child, report);
        }
    }
}

fn compare_nodes_recursive(
    old_node: &DirContextTreeNode,
    new_node: &DirContextTreeNode,
    report: &mut DiffReport,
) {
    // 1. Usporedba samog čvora (Modified check)
    // Ovdje provjeravamo samo ako su oba fajlovi. Direktoriji se ne "mijenjaju" na isti način,
    // promjene direktorija se reflektiraju kroz djecu.
    if old_node.node_type == NodeType::File && new_node.node_type == NodeType::File {
        let is_modified = is_file_modified(old_node, new_node);
        
        if is_modified {
            report.entries.push(DiffEntry {
                change_type: ChangeType::Modified,
                node_type: NodeType::File,
                relative_path: new_node.relative_path.clone(),
            });
            report.stats.files_modified += 1;
        } else {
            // Nije modificiran -> Unchanged
            report.stats.files_unchanged += 1;
        }
    } else if old_node.node_type != new_node.node_type {
        // Tip se promijenio (npr. bio file, sad je dir).
        // Tretiramo ovo kao Remove starog i Add novog.
        // Ali u praksi, pošto recursion ide po imenima, ovo će se vjerojatno riješiti kroz children logiku.
        // Ako smo ovdje, znači da smo matchali po imenu/pathu.
        
        // Logika: Stari file je maknut, novi dir je dodan.
        // Ali čekaj, ova funkcija prima dva čvora za koja ZNAMO da su "isti" path.
        // Za jednostavnost, ako se tip promijeni, sve ispod starog je removed, sve ispod novog je added.
        report.entries.push(DiffEntry { change_type: ChangeType::Removed, node_type: old_node.node_type, relative_path: old_node.relative_path.clone() });
        report.entries.push(DiffEntry { change_type: ChangeType::Added, node_type: new_node.node_type, relative_path: new_node.relative_path.clone() });
        // TODO: update stats
        return; 
    }

    // 2. Usporedba djece (samo za direktorije)
    if old_node.node_type == NodeType::Directory && new_node.node_type == NodeType::Directory {
        // Kreiramo mapu stare djece za brzi lookup po imenu (basename ili relative path)
        // Relative path je jedinstven unutar stabla, ali unutar node.children je dovoljno gledati file_name
        // Međutim, u DirContextTreeNode imamo samo relative_path. 
        // Koristit ćemo relative_path kao ključ.
        
        let mut old_children_map: HashMap<&String, &DirContextTreeNode> = HashMap::new();
        for child in &old_node.children {
            old_children_map.insert(&child.relative_path, child);
        }

        // Prolazimo kroz novu djecu
        for new_child in &new_node.children {
            if let Some(old_child) = old_children_map.remove(&new_child.relative_path) {
                // Postoji i u starom -> Rekurzivna usporedba
                compare_nodes_recursive(old_child, new_child, report);
            } else {
                // Ne postoji u starom -> ADDED
                mark_all_as_added_recursive(new_child, report);
            }
        }

        // Što je ostalo u mapi, to je REMOVED
        // Sortiranje nije nužno za logiku, ali je lijepo za determinizam (hashmap je random iter)
        // Ali DiffEntry vector će ionako biti miksan.
        for (_, old_child) in old_children_map {
            mark_all_as_removed_recursive(old_child, report);
        }
    }
}

/// Logika za detekciju promjene sadržaja.
/// Trenutno koristi Size + Timestamp.
/// U budućnosti ovdje možemo dodati Hash check.
fn is_file_modified(old: &DirContextTreeNode, new: &DirContextTreeNode) -> bool {
    // 1. Provjera veličine
    if old.content_size != new.content_size {
        return true;
    }

    // 2. Provjera vremena modifikacije
    // Napomena: Ovo može biti nepouzdano ako se kopira na drugi FS.
    // Ali za lokalni diff je standard.
    if old.last_modified_timestamp != new.last_modified_timestamp {
        return true;
    }

    false
}

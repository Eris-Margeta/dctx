use crate::config::LLM_CONTEXT_EXTENSION;
use crate::datatypes::DirContextTreeNode;
use crate::reader::read_dircontxt_file;
use std::fs;
use std::path::Path;
use std::time::SystemTime;

/// Rezultat pokušaja učitavanja stanja.
pub enum StateLoadResult {
    Loaded(DirContextTreeNode), // Uspješno učitano prethodno stanje
    FreshV1,                    // Nema prethodnog stanja (ili je resetirano), krećemo od V1
}

/// Glavna funkcija koja pokušava učitati stanje, a ako detektira problem,
/// radi "Hard Reset" (briše sve outpute) i vraća FreshV1.
pub fn load_or_reset_state(
    target_dir: &Path,
    binary_path: &Path,
    output_txt_dir: Option<&Path>,
    project_name_for_txt: &str,
) -> StateLoadResult {
    // 1. Ako binary ne postoji, to je čisti start.
    if !binary_path.exists() {
        return StateLoadResult::FreshV1;
    }

    // 2. Pokušaj učitati binary
    match read_dircontxt_file(binary_path) {
        Ok(loaded_root) => {
            // 3. Provjera integriteta "Recreated Folder"
            if is_folder_recreated(target_dir, &loaded_root) {
                eprintln!("[INFO] Detected recreated target folder. Resetting state to V1.");
                perform_full_cleanup(binary_path, output_txt_dir, project_name_for_txt);
                return StateLoadResult::FreshV1;
            }

            // Sve OK
            StateLoadResult::Loaded(loaded_root)
        }
        Err(e) => {
            // 4. Detekcija korupcije
            eprintln!("[WARN] Corrupted or invalid state file detected: {}", e);
            eprintln!("[INFO] Performing automatic cleanup and resetting to V1.");

            perform_full_cleanup(binary_path, output_txt_dir, project_name_for_txt);
            StateLoadResult::FreshV1
        }
    }
}

/// Provjerava da li se datum kreiranja/modifikacije ciljanog foldera
/// ne slaže s onim što imamo u povijesti.
fn is_folder_recreated(target_dir: &Path, _loaded_root: &DirContextTreeNode) -> bool {
    // Pokušavamo dobiti metadata s diska
    if let Ok(metadata) = fs::metadata(target_dir) {
        if let Ok(mtime) = metadata.modified() {
            let _disk_ts = mtime
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();

            // TODO: Implementirati logiku usporedbe s loaded_root.last_modified_timestamp
            // Za sada vraćamo false (pretpostavljamo da je folder isti)
            return false;
        }
    }
    // Ako ne možemo pročitati disk (npr. permission), vjerojatno će walker ionako failati kasnije.
    false
}

/// Briše binary file I sve output text fajlove koji pripadaju tom projektu.
fn perform_full_cleanup(binary_path: &Path, output_txt_dir: Option<&Path>, project_name: &str) {
    // 1. Obriši binary file (.dctx)
    if binary_path.exists() {
        if let Err(e) = fs::remove_file(binary_path) {
            eprintln!(
                "[ERROR] Failed to delete corrupted binary {}: {}",
                binary_path.display(),
                e
            );
        } else {
            eprintln!("[INFO] Deleted old binary state.");
        }
    }

    // 2. Obriši tekstualne outpute (.llmcontext.txt i diffove)
    if let Some(out_dir) = output_txt_dir {
        if !out_dir.exists() {
            return;
        }

        // Definiramo uzorke za brisanje
        let main_file_name = format!("{}.{}", project_name, LLM_CONTEXT_EXTENSION);
        let diff_prefix = format!("{}.{}-", project_name, LLM_CONTEXT_EXTENSION); // npr "MyProj.llmcontext.txt-"

        // Čitamo direktorij
        if let Ok(entries) = fs::read_dir(out_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Some(name_os) = path.file_name() {
                    let name = name_os.to_string_lossy();

                    // --- OVDJE JE BIO FIX (let mut umjesto bool) ---
                    let mut should_delete = false;

                    // A) Glavni fajl
                    if name == main_file_name {
                        should_delete = true;
                    }
                    // B) Diff fajlovi (provjera prefiksa i sufiksa)
                    else if name.starts_with(&diff_prefix) && name.ends_with("-diff.txt") {
                        should_delete = true;
                    }

                    if should_delete {
                        if let Err(e) = fs::remove_file(&path) {
                            eprintln!(
                                "[WARN] Failed to delete output file {}: {}",
                                path.display(),
                                e
                            );
                        } else {
                            eprintln!("[INFO] Cleaned up: {}", name);
                        }
                    }
                }
            }
        }
    }
}

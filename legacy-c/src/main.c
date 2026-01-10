#define _POSIX_C_SOURCE 200809L // For open_memstream
#include <libgen.h>             // For basename()
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h> // For stat() used in file_exists

#include "config.h"
#include "datatypes.h"
#include "dctx_reader.h"
#include "diff.h"
#include "ignore.h"
#include "llm_formatter.h"
#include "platform.h"
#include "utils.h"
#include "version.h"
#include "walker.h"
#include "writer.h"

// --- Constants ---
#define APP_NAME "dctx"
#define APP_VERSION "0.1.1"

// --- Function Declarations ---
static void print_usage(void);
static bool file_exists(const char *filepath);
static bool determine_output_filepaths(
    const char *target_dir_abs_path, char *dctx_output_filepath_out,
    size_t dctx_buffer_size, char *llm_output_filepath_out,
    size_t llm_buffer_size, char *diff_filepath_out, size_t diff_buffer_size,
    const char *version_string);

// --- Main Function ---
int main(int argc, char *argv[]) {
  AppConfig config;
  load_app_config(&config);

  log_info("%s v%s starting.", APP_NAME, APP_VERSION);

  // --- Argument Parsing ---
  if (argc < 2 || argc > 3 ||
      (strcmp(argv[1], "-h") == 0 || strcmp(argv[1], "--help") == 0)) {
    print_usage();
    return EXIT_SUCCESS;
  }
  if (strcmp(argv[1], "-v") == 0 || strcmp(argv[1], "--version") == 0) {
    printf("%s v%s\n", APP_NAME, APP_VERSION);
    return EXIT_SUCCESS;
  }

  const char *target_dir_arg = argv[1];
  bool copy_to_clipboard = false;
  if (argc == 3) {
    if (strcmp(argv[2], "-c") == 0 || strcmp(argv[2], "--clipboard") == 0) {
      copy_to_clipboard = true;
    } else {
      log_error("Unrecognized option: %s", argv[2]);
      print_usage();
      return EXIT_FAILURE;
    }
  }

  // --- 1. Path Resolution and Initial Setup ---
  char target_dir_abs_path[MAX_PATH_LEN];
  char dctx_filepath[MAX_PATH_LEN];
  char llm_txt_filepath[MAX_PATH_LEN];
  char diff_filepath[MAX_PATH_LEN];

  if (!platform_resolve_path(target_dir_arg, target_dir_abs_path,
                             MAX_PATH_LEN)) {
    log_error("Failed to resolve target directory path: %s", target_dir_arg);
    return EXIT_FAILURE;
  }
  log_info("Target directory resolved to: %s", target_dir_abs_path);

  // --- 2. Versioning Logic ---
  char old_version[32] = {0};
  char new_version[32] = {0};
  DirContextTreeNode *old_tree = NULL;

  determine_output_filepaths(target_dir_abs_path, dctx_filepath, MAX_PATH_LEN,
                             llm_txt_filepath, MAX_PATH_LEN, diff_filepath,
                             MAX_PATH_LEN, "");

  if (file_exists(llm_txt_filepath) && file_exists(dctx_filepath)) {
    log_info("Existing context and binary files found. Running in update/diff "
             "mode.");
    if (!parse_version_from_file(llm_txt_filepath, old_version,
                                 sizeof(old_version))) {
      log_error("Could not parse version. Starting over with V1.");
      safe_strncpy(old_version, "V1", sizeof(old_version));
    }
    calculate_next_version(old_version, new_version, sizeof(new_version));

    log_info("Loading previous state from %s", dctx_filepath);
    uint64_t old_data_offset;
    if (!dctx_read_and_parse_header(dctx_filepath, &old_tree,
                                    &old_data_offset)) {
      log_error("Failed to read previous binary file. Old state ignored.");
      old_tree = NULL;
    }
  } else {
    if (file_exists(llm_txt_filepath) && !file_exists(dctx_filepath)) {
      log_info("Warning: Text file found but required binary archive is "
               "missing. Cannot perform diff.");
    }
    log_info("Creating new V1 snapshot.");
    safe_strncpy(new_version, "V1", sizeof(new_version));
    safe_strncpy(old_version, "V1", sizeof(old_version));
  }

  log_info("Current context version will be: %s", new_version);

  determine_output_filepaths(target_dir_abs_path, dctx_filepath, MAX_PATH_LEN,
                             llm_txt_filepath, MAX_PATH_LEN, diff_filepath,
                             MAX_PATH_LEN, new_version);

  // --- 3. Scan Current Directory State ---
  IgnoreRule *ignore_rules = NULL;
  int ignore_rule_count = 0;
  if (!load_ignore_rules(target_dir_abs_path,
                         platform_get_basename(dctx_filepath), &ignore_rules,
                         &ignore_rule_count)) {
    log_error("Failed to load ignore rules.");
    if (old_tree)
      free_tree_recursive(old_tree);
    return EXIT_FAILURE;
  }

  int processed_items = 0;
  DirContextTreeNode *new_tree = walk_directory_and_build_tree(
      target_dir_abs_path, ignore_rules, ignore_rule_count, &processed_items);
  if (new_tree == NULL) {
    log_error("Failed to walk directory and build new tree.");
    if (old_tree)
      free_tree_recursive(old_tree);
    free_ignore_rules_array(ignore_rules, ignore_rule_count);
    return EXIT_FAILURE;
  }
  // NOTE: The log message for walk completion is now only in walker.c

  // --- 4. Overwrite Binary and Generate Diff ---
  int exit_code = EXIT_SUCCESS;

  log_info("Writing binary archive to: %s", dctx_filepath);
  if (!write_dircontxt_file(dctx_filepath, new_tree)) {
    log_error("Failed to write the .dircontxt binary file. Cannot proceed.");
    exit_code = EXIT_FAILURE;
    goto cleanup;
  }

  if (old_tree != NULL) {
    log_info("Comparing new state to previous state...");
    DiffReport *report = compare_trees(old_tree, new_tree);
    if (report && report->has_changes &&
        !copy_to_clipboard) { // Dont generate diff file for clipboard
      log_info("Changes detected. Generating diff file: %s", diff_filepath);
      uint64_t new_data_offset = 0;
      DirContextTreeNode *temp_tree_for_diff = NULL;
      if (dctx_read_and_parse_header(dctx_filepath, &temp_tree_for_diff,
                                     &new_data_offset)) {
        generate_diff_file(diff_filepath, report, temp_tree_for_diff,
                           dctx_filepath, new_data_offset, old_version,
                           new_version);
        free_tree_recursive(temp_tree_for_diff);
      }
    } else {
      log_info("No changes detected since version %s.", old_version);
    }
    free_diff_report(report);
  }

  // --- 5. Generate Text Output based on Config ---
  if (copy_to_clipboard) {
    log_info("Generating LLM context and copying to clipboard...");
    uint64_t final_data_offset = 0;
    DirContextTreeNode *final_tree_for_llm = NULL;

    if (!dctx_read_and_parse_header(dctx_filepath, &final_tree_for_llm,
                                    &final_data_offset)) {
      log_error(
          "Failed to read back binary. Cannot generate clipboard content.");
      exit_code = EXIT_FAILURE;
    } else {
      char *clipboard_buffer = NULL;
      size_t buffer_size = 0;
      FILE *mem_stream = open_memstream(&clipboard_buffer, &buffer_size);

      if (mem_stream == NULL) {
        log_error("Failed to create in-memory stream for clipboard.");
        exit_code = EXIT_FAILURE;
      } else {
        bool gen_success = generate_llm_context_to_stream(
            mem_stream, final_tree_for_llm, dctx_filepath, final_data_offset,
            new_version);

        fclose(mem_stream); // Flushes, null-terminates, sets buffer/size

        if (gen_success) {
          platform_copy_to_clipboard(clipboard_buffer);
        } else {
          log_error("Failed to generate content for clipboard.");
          exit_code = EXIT_FAILURE;
        }
        free(clipboard_buffer); // Must free buffer from open_memstream
      }
      free_tree_recursive(final_tree_for_llm);
    }
    // No-trace cleanup for clipboard mode
    remove(dctx_filepath);
    log_info("Clipboard mode: Removed binary file %s.", dctx_filepath);

  } else if (config.output_mode == OUTPUT_MODE_BINARY_ONLY) {
    log_info("Skipping text file generation as per binary-only mode.");
    if (file_exists(llm_txt_filepath))
      remove(llm_txt_filepath);
    if (file_exists(diff_filepath))
      remove(diff_filepath);
  } else { // This covers BOTH and TEXT_ONLY modes (default file output)
    log_info("Generating LLM context file: %s", llm_txt_filepath);
    uint64_t final_data_offset = 0;
    DirContextTreeNode *final_tree_for_llm = NULL;

    if (!dctx_read_and_parse_header(dctx_filepath, &final_tree_for_llm,
                                    &final_data_offset)) {
      log_error("Failed to read back binary. Cannot generate text file.");
      exit_code = EXIT_FAILURE;
    } else {
      if (!generate_llm_context_file(llm_txt_filepath, final_tree_for_llm,
                                     dctx_filepath, final_data_offset,
                                     new_version)) {
        log_error("Failed to generate .llmcontext.txt file.");
        exit_code = EXIT_FAILURE;
      }
      free_tree_recursive(final_tree_for_llm);
    }
  }

cleanup:
  // --- 6. Final Memory Free ---
  if (old_tree)
    free_tree_recursive(old_tree);
  if (new_tree)
    free_tree_recursive(new_tree);
  free_ignore_rules_array(ignore_rules, ignore_rule_count);

  log_info("dctx run finished.");
  return exit_code;
}

static void print_usage(void) {
  printf("Usage: %s <target_directory> [options]\n", APP_NAME);
  printf("Creates a versioned context snapshot of the specified directory.\n");
  printf("Behavior is controlled by ~/.config/dircontxt/config\n\n");
  printf("Options:\n");
  printf("  -c, --clipboard  Copy the context to the clipboard instead of "
         "writing a file.\n");
  printf("                   This leaves no files behind.\n");
  printf("  -h, --help       Show this help message and exit.\n");
  printf("  -v, --version    Show version information and exit.\n");
}

static bool file_exists(const char *filepath) {
  if (filepath == NULL || filepath[0] == '\0')
    return false;
  struct stat buffer;
  return (stat(filepath, &buffer) == 0);
}

static bool determine_output_filepaths(
    const char *target_dir_abs_path, char *dctx_output_filepath_out,
    size_t dctx_buffer_size, char *llm_output_filepath_out,
    size_t llm_buffer_size, char *diff_filepath_out, size_t diff_buffer_size,
    const char *version_string) {
  char *target_basename = get_directory_basename(target_dir_abs_path);
  if (!target_basename)
    return false;

  char *parent_dir = platform_get_dirname(target_dir_abs_path);
  if (!parent_dir) {
    free(target_basename);
    return false;
  }

  char dctx_filename[MAX_PATH_LEN];
  snprintf(dctx_filename, MAX_PATH_LEN, "%s.dircontxt", target_basename);
  platform_join_paths(parent_dir, dctx_filename, dctx_output_filepath_out,
                      dctx_buffer_size);

  char llm_filename[MAX_PATH_LEN];
  snprintf(llm_filename, MAX_PATH_LEN, "%s.llmcontext.txt", target_basename);
  platform_join_paths(parent_dir, llm_filename, llm_output_filepath_out,
                      llm_buffer_size);

  // Diff file is only created for an update (e.g., "V1.1", "V1.2", etc.)
  if (version_string != NULL && strchr(version_string, '.') != NULL) {
    char diff_filename[MAX_PATH_LEN];
    snprintf(diff_filename, MAX_PATH_LEN, "%s.llmcontext-%s-diff.txt",
             target_basename, version_string);
    platform_join_paths(parent_dir, diff_filename, diff_filepath_out,
                        diff_buffer_size);
  } else {
    if (diff_filepath_out != NULL && diff_buffer_size > 0)
      diff_filepath_out[0] = '\0';
  }

  free(target_basename);
  free(parent_dir);
  return true;
}

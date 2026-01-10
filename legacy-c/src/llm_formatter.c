#include "llm_formatter.h"
#include "datatypes.h"
#include "dctx_reader.h"
#include "utils.h"
#include "version.h" // For version header constants

#include <ctype.h>
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <strings.h> // For strcasecmp (POSIX)
#include <time.h>

// --- Static Helper Function Declarations ---

static void write_manifest_entry_recursive(FILE *fp, DirContextTreeNode *node,
                                           int indent_level,
                                           int *shared_id_counter);
static bool write_file_content_block(FILE *fp,
                                     const DirContextTreeNode *file_node,
                                     FILE *dctx_binary_fp,
                                     uint64_t data_section_offset);
static bool is_likely_binary(const char *buffer, size_t size,
                             const char *path_for_ext_check);
static bool write_all_file_content_blocks_recursive(
    FILE *fp, const DirContextTreeNode *node, FILE *dctx_binary_fp,
    uint64_t data_section_offset);
static DirContextTreeNode *
find_node_by_path_recursive(DirContextTreeNode *node,
                            const char *relative_path);

// --- Public Function Implementations ---

// REFACTORED: This function is now a wrapper around the stream version.
bool generate_llm_context_file(const char *llm_txt_filepath,
                               DirContextTreeNode *root_node,
                               const char *dctx_binary_filepath,
                               uint64_t data_section_start_offset_in_dctx_file,
                               const char *version_string) {
  if (llm_txt_filepath == NULL) {
    log_error("llm_formatter: llm_txt_filepath is NULL.");
    return false;
  }

  FILE *llm_fp = fopen(llm_txt_filepath, "w");
  if (llm_fp == NULL) {
    log_error(
        "llm_formatter: Failed to open LLM context file '%s' for writing: %s",
        llm_txt_filepath, strerror(errno));
    return false;
  }

  bool success = generate_llm_context_to_stream(
      llm_fp, root_node, dctx_binary_filepath,
      data_section_start_offset_in_dctx_file, version_string);

  if (fclose(llm_fp) == EOF) {
    log_error("llm_formatter: Error closing LLM context file '%s': %s",
              llm_txt_filepath, strerror(errno));
    success = false;
  }

  return success;
}

// NEW: The core logic now resides here, writing to any open stream.
bool generate_llm_context_to_stream(
    FILE *output_stream, DirContextTreeNode *root_node,
    const char *dctx_binary_filepath,
    uint64_t data_section_start_offset_in_dctx_file,
    const char *version_string) {

  if (output_stream == NULL || root_node == NULL ||
      dctx_binary_filepath == NULL || version_string == NULL) {
    log_error(
        "llm_formatter: Invalid arguments for generating context stream.");
    return false;
  }

  // --- Write Header ---
  fprintf(output_stream, "%s%s%s\n\n", VERSION_HEADER_PREFIX, version_string,
          VERSION_HEADER_SUFFIX);
  fprintf(output_stream, "<INSTRUCTIONS>\n");
  fprintf(output_stream,
          "1. Manifest: The \"DIRECTORY_TREE\" section below lists all "
          "files and directories.\n");
  fprintf(output_stream, "   - Each entry: [TYPE] RELATIVE_PATH (ID:UNIQUE_ID, "
                         "MOD:UNIX_TIMESTAMP, SIZE:BYTES)\n");
  fprintf(output_stream, "   - TYPE is [D] for directory, [F] for file.\n");
  fprintf(output_stream, "   - SIZE is for files only.\n");
  fprintf(output_stream,
          "   - Binary files may be noted with (CONTENT:BINARY_HINT or "
          "CONTENT:BINARY_PLACEHOLDER).\n");
  fprintf(output_stream, "2. Content Access: To read a specific file:\n");
  fprintf(output_stream, "   - Find its UNIQUE_ID from the DIRECTORY_TREE.\n");
  fprintf(
      output_stream,
      "   - Search for the marker: <FILE_CONTENT_START ID=\"UNIQUE_ID\">\n");
  fprintf(output_stream, "   - The content is between this marker and "
                         "<FILE_CONTENT_END ID=\"UNIQUE_ID\">\n");
  fprintf(output_stream, "</INSTRUCTIONS>\n\n");

  // --- Write Directory Tree ---
  fprintf(output_stream, "<DIRECTORY_TREE>\n");
  int shared_id_counter = 1;
  write_manifest_entry_recursive(output_stream, root_node, 0,
                                 &shared_id_counter);
  fprintf(output_stream, "</DIRECTORY_TREE>\n");

  // --- Write File Contents ---
  FILE *dctx_binary_fp = fopen(dctx_binary_filepath, "rb");
  if (dctx_binary_fp == NULL) {
    log_error("llm_formatter: Failed to open .dircontxt binary '%s' for "
              "reading content: %s",
              dctx_binary_filepath, strerror(errno));
    return false;
  }

  write_all_file_content_blocks_recursive(
      output_stream, root_node, dctx_binary_fp,
      data_section_start_offset_in_dctx_file);

  fclose(dctx_binary_fp);

  // Final flush to ensure all data is written to the stream
  fflush(output_stream);

  return true;
}

bool generate_diff_file(const char *diff_filepath, const DiffReport *report,
                        DirContextTreeNode *new_root_node,
                        const char *dctx_binary_filepath,
                        uint64_t data_section_start_offset_in_dctx_file,
                        const char *old_version, const char *new_version) {

  if (diff_filepath == NULL || report == NULL || new_root_node == NULL ||
      dctx_binary_filepath == NULL) {
    log_error("llm_formatter: Invalid arguments for generating diff file.");
    return false;
  }

  FILE *diff_fp = fopen(diff_filepath, "w");
  if (diff_fp == NULL) {
    log_error("llm_formatter: Failed to open diff file '%s' for writing: %s",
              diff_filepath, strerror(errno));
    return false;
  }

  // --- Write Diff Header ---
  fprintf(diff_fp, "[DIRCONTXT_LLM_DIFF_V1]\n");
  fprintf(diff_fp, "Version Change: %s -> %s\n\n", old_version, new_version);

  // --- Write Summary of Changes ---
  fprintf(diff_fp, "<CHANGES_SUMMARY>\n");
  for (int i = 0; i < report->count; ++i) {
    const DiffEntry *entry = &report->entries[i];
    const char *type_str = "UNKNOWN";
    if (entry->type == ITEM_ADDED)
      type_str = "ADDED";
    if (entry->type == ITEM_REMOVED)
      type_str = "REMOVED";
    if (entry->type == ITEM_MODIFIED)
      type_str = "MODIFIED";

    fprintf(diff_fp, "[%s] %s%s\n", type_str, entry->relative_path,
            (entry->node_type == NODE_TYPE_DIRECTORY ? "/" : ""));
  }
  fprintf(diff_fp, "</CHANGES_SUMMARY>\n\n");

  // --- Write the NEW Directory Tree ---
  fprintf(diff_fp, "<UPDATED_DIRECTORY_TREE>\n");
  int shared_id_counter = 1; // Reset counter for the new tree
  write_manifest_entry_recursive(diff_fp, new_root_node, 0, &shared_id_counter);
  fprintf(diff_fp, "</UPDATED_DIRECTORY_TREE>\n");

  // --- Write Content of ADDED and MODIFIED Files ---
  FILE *dctx_binary_fp = fopen(dctx_binary_filepath, "rb");
  if (dctx_binary_fp == NULL) {
    log_error("llm_formatter (diff): Failed to open .dircontxt binary '%s' for "
              "reading content: %s",
              dctx_binary_filepath, strerror(errno));
    fclose(diff_fp);
    return false;
  }

  for (int i = 0; i < report->count; ++i) {
    const DiffEntry *entry = &report->entries[i];
    if ((entry->type == ITEM_ADDED || entry->type == ITEM_MODIFIED) &&
        entry->node_type == NODE_TYPE_FILE) {
      DirContextTreeNode *node_to_write =
          find_node_by_path_recursive(new_root_node, entry->relative_path);
      if (node_to_write) {
        write_file_content_block(diff_fp, node_to_write, dctx_binary_fp,
                                 data_section_start_offset_in_dctx_file);
      }
    }
  }

  fclose(dctx_binary_fp);

  // --- Finalize and Close ---
  bool success = true;
  if (fclose(diff_fp) == EOF) {
    log_error("llm_formatter: Error closing diff file '%s': %s", diff_filepath,
              strerror(errno));
    success = false;
  }
  return success;
}

// --- Static Helper Function Implementations (NO CHANGES BELOW THIS LINE) ---

static void write_manifest_entry_recursive(FILE *fp, DirContextTreeNode *node,
                                           int indent_level,
                                           int *shared_id_counter) {
  if (node == NULL)
    return;

  for (int i = 0; i < indent_level; ++i)
    fprintf(fp, "  ");

  if (node->type == NODE_TYPE_DIRECTORY) {
    if (indent_level == 0) {
      strcpy(node->generated_id_for_llm, "ROOT");
    } else {
      snprintf(node->generated_id_for_llm, sizeof(node->generated_id_for_llm),
               "D%03d", (*shared_id_counter)++);
    }
    fprintf(fp, "[D] %s (ID:%s, MOD:%lld)\n", node->relative_path,
            node->generated_id_for_llm,
            (long long)node->last_modified_timestamp);
    for (uint32_t i = 0; i < node->num_children; ++i) {
      write_manifest_entry_recursive(fp, node->children[i], indent_level + 1,
                                     shared_id_counter);
    }
  } else { // NODE_TYPE_FILE
    snprintf(node->generated_id_for_llm, sizeof(node->generated_id_for_llm),
             "F%03d", (*shared_id_counter)++);
    fprintf(fp, "[F] %s (ID:%s, MOD:%lld, SIZE:%lld", node->relative_path,
            node->generated_id_for_llm,
            (long long)node->last_modified_timestamp,
            (long long)node->content_size);

    if (is_likely_binary(NULL, 0, node->relative_path)) {
      fprintf(fp, ", CONTENT:BINARY_HINT");
    }
    fprintf(fp, ")\n");
  }
}

static bool write_file_content_block(FILE *fp,
                                     const DirContextTreeNode *file_node,
                                     FILE *dctx_binary_fp,
                                     uint64_t data_section_offset) {
  if (file_node->type != NODE_TYPE_FILE)
    return true;
  if (file_node->generated_id_for_llm[0] == '\0') {
    log_error("llm_formatter: Skipping content block for file '%s' due to "
              "missing generated ID.",
              file_node->relative_path);
    return true;
  }

  fprintf(fp, "\n<FILE_CONTENT_START ID=\"%s\" PATH=\"%s\">\n",
          file_node->generated_id_for_llm, file_node->relative_path);

  if (file_node->content_size > 0) {
    char *content_buffer = (char *)malloc(file_node->content_size);
    if (content_buffer == NULL) {
      fprintf(fp, "[ERROR: Could not allocate memory to read file content]\n");
    } else {
      if (!dctx_read_file_content(dctx_binary_fp, data_section_offset,
                                  file_node, content_buffer,
                                  file_node->content_size)) {
        fprintf(
            fp,
            "[ERROR: Could not read file content from .dircontxt binary]\n");
      } else {
        if (is_likely_binary(content_buffer, file_node->content_size,
                             file_node->relative_path)) {
          fprintf(fp, "[BINARY CONTENT PLACEHOLDER - Size: %llu bytes]\n",
                  (unsigned long long)file_node->content_size);
        } else {
          fwrite(content_buffer, 1, file_node->content_size, fp);
        }
      }
      free(content_buffer);
    }
  }

  fprintf(fp, "</FILE_CONTENT_END ID=\"%s\">\n",
          file_node->generated_id_for_llm);
  return true;
}

static bool is_likely_binary(const char *buffer, size_t size,
                             const char *path_for_ext_check) {
  // --- Check 1: By file extension ---
  const char *binary_exts[] = {
      ".png", ".jpg",   ".jpeg", ".gif", ".bmp",    ".ico", ".tiff", ".mp3",
      ".wav", ".flac",  ".ogg",  ".mp4", ".mov",    ".avi", ".mkv",  ".pdf",
      ".zip", ".gz",    ".tar",  ".rar", ".7z",     ".bz2", ".exe",  ".dll",
      ".so",  ".dylib", ".o",    ".a",   ".lib",    ".bin", ".dat",  ".iso",
      ".img", ".class", ".jar",  ".pyc", ".sqlite", ".db"};
  const char *ext = strrchr(path_for_ext_check, '.');
  if (ext) {
    for (size_t i = 0; i < sizeof(binary_exts) / sizeof(binary_exts[0]); ++i) {
      if (strcasecmp(ext, binary_exts[i]) == 0) {
        return true;
      }
    }
  }

  // --- Check 2: By content (if buffer is provided) ---
  if (buffer == NULL || size == 0) {
    return false; // Cannot check content, rely on extension check result
  }

  // Contains null bytes (a strong indicator)
  if (memchr(buffer, '\0', size) != NULL) {
    return true;
  }

  // High percentage of non-printable ASCII characters
  int non_printable = 0;
  size_t check_len = size < 512 ? size : 512;
  for (size_t i = 0; i < check_len; i++) {
    if (!isprint((unsigned char)buffer[i]) &&
        !isspace((unsigned char)buffer[i])) {
      non_printable++;
    }
  }
  if (check_len > 0 && (double)non_printable / check_len > 0.2) { // Over 20%
    return true;
  }

  return false;
}

static bool write_all_file_content_blocks_recursive(
    FILE *fp, const DirContextTreeNode *node, FILE *dctx_binary_fp,
    uint64_t data_section_offset) {
  if (node == NULL)
    return true;
  if (node->type == NODE_TYPE_FILE) {
    write_file_content_block(fp, node, dctx_binary_fp, data_section_offset);
  } else if (node->type == NODE_TYPE_DIRECTORY) {
    for (uint32_t i = 0; i < node->num_children; ++i) {
      write_all_file_content_blocks_recursive(
          fp, node->children[i], dctx_binary_fp, data_section_offset);
    }
  }
  return true;
}

static DirContextTreeNode *
find_node_by_path_recursive(DirContextTreeNode *node,
                            const char *relative_path) {
  if (node == NULL)
    return NULL;

  if (strcmp(node->relative_path, relative_path) == 0) {
    return node;
  }

  if (node->type == NODE_TYPE_DIRECTORY) {
    for (uint32_t i = 0; i < node->num_children; ++i) {
      if (strncmp(relative_path, node->children[i]->relative_path,
                  strlen(node->children[i]->relative_path)) == 0) {
        DirContextTreeNode *found =
            find_node_by_path_recursive(node->children[i], relative_path);
        if (found)
          return found;
      }
    }
  }

  return NULL;
}

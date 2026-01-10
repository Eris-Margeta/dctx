#include "writer.h"
#include "platform.h" // For platform_join_paths, etc. (if needed, though mostly paths are in nodes)
#include "utils.h" // For log_info, log_error, log_debug, safe_strncpy

#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

// --- Static Helper Function Declarations ---

// Pass 1: Recursively traverses the tree. For files, it copies their content to
// data_stream,
//         updates their content_offset and content_size in the node, and
//         accumulates the total data offset.
static bool collect_file_data_and_update_nodes_recursive(
    DirContextTreeNode *node,
    FILE *data_stream, /* Temp file for concatenated file data */
    uint64_t *current_data_offset_accumulator);

// Pass 2: Recursively traverses the tree (now with updated file nodes) and
// serializes
//         each node's metadata to the header_stream.
static bool
serialize_header_recursive(const DirContextTreeNode *node,
                           FILE *header_stream); /* Temp file for header data */

// Helper to write a single node's metadata to the header stream
static bool serialize_single_node(const DirContextTreeNode *node,
                                  FILE *header_stream);

// Helper to copy content from one file stream to another
static bool copy_stream_content(FILE *dest, FILE *src);

// --- Implementation of Static Helper Functions ---

static bool collect_file_data_and_update_nodes_recursive(
    DirContextTreeNode *node, FILE *data_stream,
    uint64_t *current_data_offset_accumulator) {
  if (node == NULL)
    return true; // Base case for recursion

  if (node->type == NODE_TYPE_FILE) {
    node->content_offset_in_data_section = *current_data_offset_accumulator;
    node->content_size = 0; // Initialize size

    FILE *src_file = fopen(node->disk_path, "rb"); // disk_path is absolute
    if (src_file == NULL) {
      log_error("Failed to open source file %s for reading: %s",
                node->disk_path, strerror(errno));
      // Decide how to handle: skip file (size 0) or abort? Let's skip.
      return true; // Continue with other files
    }

    log_debug("Writing data for file: %s (offset: %llu)", node->relative_path,
              (unsigned long long)node->content_offset_in_data_section);

    // Copy content and count bytes
    int c;
    size_t bytes_written_for_this_file = 0;
    while ((c = fgetc(src_file)) != EOF) {
      if (fputc(c, data_stream) == EOF) {
        log_error("Failed to write data to temporary data stream for %s: %s",
                  node->disk_path, strerror(errno));
        fclose(src_file);
        return false; // Critical error
      }
      bytes_written_for_this_file++;
    }

    if (ferror(src_file)) {
      log_error("Error reading from source file %s: %s", node->disk_path,
                strerror(errno));
      // Continue, but size might be incomplete
    }
    fclose(src_file);

    node->content_size = bytes_written_for_this_file;
    *current_data_offset_accumulator += node->content_size;

    log_debug("Finished data for file: %s (size: %llu, new total offset: %llu)",
              node->relative_path, (unsigned long long)node->content_size,
              (unsigned long long)*current_data_offset_accumulator);

  } else if (node->type == NODE_TYPE_DIRECTORY) {
    for (uint32_t i = 0; i < node->num_children; ++i) {
      if (!collect_file_data_and_update_nodes_recursive(
              node->children[i], data_stream,
              current_data_offset_accumulator)) {
        return false; // Propagate error
      }
    }
  }
  return true;
}

static bool serialize_single_node(const DirContextTreeNode *node,
                                  FILE *header_stream) {
  // 1. Node Type (1 byte)
  uint8_t node_type_byte = (uint8_t)node->type;
  if (fwrite(&node_type_byte, sizeof(uint8_t), 1, header_stream) != 1)
    return false;

  // 2. Full Relative Path Length (uint16_t, 2 bytes)
  uint16_t path_len = (uint16_t)strlen(node->relative_path);
  if (fwrite(&path_len, sizeof(uint16_t), 1, header_stream) != 1)
    return false;

  // 3. Full Relative Path (Variable length, UTF-8)
  if (path_len > 0) {
    if (fwrite(node->relative_path, sizeof(char), path_len, header_stream) !=
        path_len)
      return false;
  }

  // 4. Last Modified Timestamp (uint64_t, 8 bytes)
  if (fwrite(&node->last_modified_timestamp, sizeof(uint64_t), 1,
             header_stream) != 1)
    return false;

  if (node->type == NODE_TYPE_FILE) {
    // 5. Content Offset in Data Section (uint64_t, 8 bytes)
    if (fwrite(&node->content_offset_in_data_section, sizeof(uint64_t), 1,
               header_stream) != 1)
      return false;
    // 6. Content Size (uint64_t, 8 bytes)
    if (fwrite(&node->content_size, sizeof(uint64_t), 1, header_stream) != 1)
      return false;
  } else if (node->type == NODE_TYPE_DIRECTORY) {
    // 5. Number of Children (uint32_t, 4 bytes)
    if (fwrite(&node->num_children, sizeof(uint32_t), 1, header_stream) != 1)
      return false;
  }
  return true;
}

static bool serialize_header_recursive(const DirContextTreeNode *node,
                                       FILE *header_stream) {
  if (node == NULL)
    return true; // Base case

  // Write current node's metadata (Pre-order traversal for header)
  log_debug("Serializing header for: %s (type: %d)", node->relative_path,
            node->type);
  if (!serialize_single_node(node, header_stream)) {
    log_error("Failed to serialize node data for %s to header stream.",
              node->relative_path);
    return false;
  }

  if (node->type == NODE_TYPE_DIRECTORY) {
    for (uint32_t i = 0; i < node->num_children; ++i) {
      if (!serialize_header_recursive(node->children[i], header_stream)) {
        return false; // Propagate error
      }
    }
  }
  return true;
}

static bool copy_stream_content(FILE *dest, FILE *src) {
  rewind(src); // Ensure we start from the beginning of the source stream
  int c;
  while ((c = fgetc(src)) != EOF) {
    if (fputc(c, dest) == EOF) {
      log_error("Failed to copy stream content: fputc error: %s",
                strerror(errno));
      return false;
    }
  }
  if (ferror(src)) {
    log_error("Failed to copy stream content: ferror on src: %s",
              strerror(errno));
    return false;
  }
  return true;
}

// --- Public Function Implementation ---

bool write_dircontxt_file(const char *output_filepath,
                          DirContextTreeNode *root_node) {
  if (output_filepath == NULL || root_node == NULL) {
    log_error("Output filepath or root node is NULL.");
    return false;
  }

  FILE *header_temp_fp = NULL;
  FILE *data_temp_fp = NULL;
  FILE *output_fp = NULL;
  bool success = false;

  // Use tmpfile() to create temporary files that are automatically deleted on
  // close or program termination.
  header_temp_fp = tmpfile();
  if (header_temp_fp == NULL) {
    log_error("Failed to create temporary file for header: %s",
              strerror(errno));
    goto cleanup;
  }

  data_temp_fp = tmpfile();
  if (data_temp_fp == NULL) {
    log_error("Failed to create temporary file for data: %s", strerror(errno));
    goto cleanup;
  }

  // Pass 1: Collect all file data into data_temp_fp and update node
  // offsets/sizes
  log_info("Pass 1: Collecting file data...");
  uint64_t total_data_offset = 0;
  if (!collect_file_data_and_update_nodes_recursive(root_node, data_temp_fp,
                                                    &total_data_offset)) {
    log_error("Failed during file data collection pass.");
    goto cleanup;
  }
  log_info(
      "Pass 1: File data collection complete. Total data size: %llu bytes.",
      (unsigned long long)total_data_offset);
  fflush(data_temp_fp); // Ensure all data is written to the temp file

  // Pass 2: Serialize the header (tree structure) to header_temp_fp
  log_info("Pass 2: Serializing header data...");
  if (!serialize_header_recursive(root_node, header_temp_fp)) {
    log_error("Failed during header serialization pass.");
    goto cleanup;
  }
  log_info("Pass 2: Header data serialization complete.");
  fflush(header_temp_fp); // Ensure all header data is written

  // Now, assemble the final file
  output_fp = fopen(output_filepath, "wb");
  if (output_fp == NULL) {
    log_error("Failed to open output file %s for writing: %s", output_filepath,
              strerror(errno));
    goto cleanup;
  }

  log_info("Assembling final file: %s", output_filepath);

  // 1. Write Signature
  if (fwrite(DIRCONTXT_FILE_SIGNATURE, 1, DIRCONTXT_SIGNATURE_LEN, output_fp) !=
      DIRCONTXT_SIGNATURE_LEN) {
    log_error("Failed to write file signature to %s.", output_filepath);
    goto cleanup;
  }

  // 2. Write Header Section (from header_temp_fp)
  if (!copy_stream_content(output_fp, header_temp_fp)) {
    log_error("Failed to copy header temp content to output file %s.",
              output_filepath);
    goto cleanup;
  }

  // 3. Write Data Section (from data_temp_fp)
  if (!copy_stream_content(output_fp, data_temp_fp)) {
    log_error("Failed to copy data temp content to output file %s.",
              output_filepath);
    goto cleanup;
  }

  log_info("Successfully wrote .dircontxt file: %s", output_filepath);
  success = true;

cleanup:
  if (header_temp_fp != NULL)
    fclose(header_temp_fp); // tmpfile() handles deletion
  if (data_temp_fp != NULL)
    fclose(data_temp_fp); // tmpfile() handles deletion
  if (output_fp != NULL) {
    if (fclose(output_fp) == EOF &&
        success) { // Only log fclose error if we thought we succeeded
      log_error("Error closing output file %s: %s", output_filepath,
                strerror(errno));
      success = false; // An error on close means it might not be fully
                       // written/flushed
    }
  }
  if (!success &&
      output_filepath !=
          NULL) { // If something went wrong, try to delete partial output file
                  // remove(output_filepath); // Optionally remove on failure
  }

  return success;
}

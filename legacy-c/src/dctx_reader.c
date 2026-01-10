#include "dctx_reader.h"
#include "platform.h" // For platform_get_mod_time (though not strictly needed here as it's read from file)
#include "utils.h" // For create_node, add_child_to_parent_node, log_error, log_debug, safe_strncpy
#include "writer.h" // For DIRCONTXT_FILE_SIGNATURE, DIRCONTXT_SIGNATURE_LEN

#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

// --- Static Helper Function Declarations ---

// Reads a single node's metadata from the file stream and populates a new
// DirContextTreeNode. It does NOT handle reading children for directory nodes;
// that's done by the recursive caller.
static DirContextTreeNode *read_single_node_metadata(FILE *fp);

// Recursively reads child nodes for a directory node.
static bool
read_children_for_directory_node(FILE *fp, DirContextTreeNode *parent_dir_node);

// --- Implementation of Static Helper Functions ---

static DirContextTreeNode *read_single_node_metadata(FILE *fp) {
  DirContextTreeNode temp_node_data; // Temporary stack storage to read into
  memset(&temp_node_data, 0, sizeof(DirContextTreeNode));

  // 1. Node Type (1 byte)
  uint8_t node_type_byte;
  if (fread(&node_type_byte, sizeof(uint8_t), 1, fp) != 1) {
    log_error("dctx_reader: Failed to read node type: %s",
              feof(fp) ? "EOF" : strerror(errno));
    return NULL;
  }
  temp_node_data.type = (NodeType)node_type_byte;

  // 2. Full Relative Path Length (uint16_t, 2 bytes)
  uint16_t path_len;
  if (fread(&path_len, sizeof(uint16_t), 1, fp) != 1) {
    log_error("dctx_reader: Failed to read path length: %s",
              feof(fp) ? "EOF" : strerror(errno));
    return NULL;
  }

  // 3. Full Relative Path (Variable length, UTF-8)
  if (path_len > MAX_PATH_LEN - 1) { // -1 for null terminator
    log_error("dctx_reader: Path length %u exceeds MAX_PATH_LEN %d.", path_len,
              MAX_PATH_LEN);
    return NULL;
  }
  if (path_len > 0) {
    if (fread(temp_node_data.relative_path, sizeof(char), path_len, fp) !=
        path_len) {
      log_error("dctx_reader: Failed to read path string: %s",
                feof(fp) ? "EOF" : strerror(errno));
      return NULL;
    }
  }
  temp_node_data.relative_path[path_len] = '\0'; // Ensure null termination

  // 4. Last Modified Timestamp (uint64_t, 8 bytes)
  if (fread(&temp_node_data.last_modified_timestamp, sizeof(uint64_t), 1, fp) !=
      1) {
    log_error(
        "dctx_reader: Failed to read last modified timestamp for '%s': %s",
        temp_node_data.relative_path, feof(fp) ? "EOF" : strerror(errno));
    return NULL;
  }

  if (temp_node_data.type == NODE_TYPE_FILE) {
    // 5. Content Offset in Data Section (uint64_t, 8 bytes)
    if (fread(&temp_node_data.content_offset_in_data_section, sizeof(uint64_t),
              1, fp) != 1) {
      log_error("dctx_reader: Failed to read content offset for file '%s': %s",
                temp_node_data.relative_path,
                feof(fp) ? "EOF" : strerror(errno));
      return NULL;
    }
    // 6. Content Size (uint64_t, 8 bytes)
    if (fread(&temp_node_data.content_size, sizeof(uint64_t), 1, fp) != 1) {
      log_error("dctx_reader: Failed to read content size for file '%s': %s",
                temp_node_data.relative_path,
                feof(fp) ? "EOF" : strerror(errno));
      return NULL;
    }
  } else if (temp_node_data.type == NODE_TYPE_DIRECTORY) {
    // 5. Number of Children (uint32_t, 4 bytes)
    if (fread(&temp_node_data.num_children, sizeof(uint32_t), 1, fp) != 1) {
      log_error("dctx_reader: Failed to read num children for dir '%s': %s",
                temp_node_data.relative_path,
                feof(fp) ? "EOF" : strerror(errno));
      return NULL;
    }
    // For directories read from file, initialize capacity for children array
    // later
    temp_node_data.children_capacity =
        temp_node_data.num_children; // We know exactly how many
  } else {
    log_error("dctx_reader: Unknown node type %d encountered for '%s'.",
              temp_node_data.type, temp_node_data.relative_path);
    return NULL;
  }

  // Allocate the actual node on the heap and copy data
  // We use create_node from utils.c as it initializes some fields, but we
  // overwrite most. The disk_path for statting is not relevant here, as we are
  // reading from archive. We can pass an empty string or its relative_path for
  // disk_path if create_node requires it.
  DirContextTreeNode *new_node =
      (DirContextTreeNode *)malloc(sizeof(DirContextTreeNode));
  if (!new_node) {
    perror("dctx_reader: malloc for new_node failed");
    return NULL;
  }
  *new_node = temp_node_data; // Copy all parsed data

  // If it's a directory and has children, allocate the children array
  if (new_node->type == NODE_TYPE_DIRECTORY && new_node->num_children > 0) {
    new_node->children = (DirContextTreeNode **)calloc(
        new_node->num_children, sizeof(DirContextTreeNode *));
    if (new_node->children == NULL) {
      perror("dctx_reader: calloc for children array failed");
      free(new_node);
      return NULL;
    }
  } else {
    new_node->children =
        NULL; // Ensure it's NULL if no children or not a directory
  }
  // disk_path is not meaningful when reading from archive, ensure it's empty or
  // handled.
  new_node->disk_path[0] = '\0';

  log_debug("dctx_reader: Read node metadata: path='%s', type=%d, mod=%llu",
            new_node->relative_path, new_node->type,
            (unsigned long long)new_node->last_modified_timestamp);
  if (new_node->type == NODE_TYPE_FILE) {
    log_debug("  File: offset=%llu, size=%llu",
              (unsigned long long)new_node->content_offset_in_data_section,
              (unsigned long long)new_node->content_size);
  } else {
    log_debug("  Dir: num_children=%u", new_node->num_children);
  }

  return new_node;
}

static bool
read_children_for_directory_node(FILE *fp,
                                 DirContextTreeNode *parent_dir_node) {
  if (parent_dir_node->type != NODE_TYPE_DIRECTORY)
    return true; // Should not happen

  for (uint32_t i = 0; i < parent_dir_node->num_children; ++i) {
    DirContextTreeNode *child_node = read_single_node_metadata(fp);
    if (child_node == NULL) {
      log_error(
          "dctx_reader: Failed to read metadata for child %u of dir '%s'.", i,
          parent_dir_node->relative_path);
      // Cleanup: free previously read children of this parent before returning
      // error
      for (uint32_t j = 0; j < i; ++j) {
        free_tree_recursive(parent_dir_node->children[j]);
      }
      // free(parent_dir_node->children); // Children array would be freed by
      // free_tree_recursive on parent parent_dir_node->num_children = 0; //
      // Mark as having no valid children parsed
      return false;
    }
    parent_dir_node->children[i] = child_node;

    // Recursively read children for this child_node if it's also a directory
    if (child_node->type == NODE_TYPE_DIRECTORY) {
      if (!read_children_for_directory_node(fp, child_node)) {
        // Error in deeper recursion. child_node and its partially read children
        // will be freed when parent_dir_node is eventually freed. To be very
        // robust, one might try to clean up more specifically here.
        return false;
      }
    }
  }
  return true;
}

// --- Public Function Implementations ---

bool dctx_read_and_parse_header(const char *dctx_filepath,
                                DirContextTreeNode **root_node_out,
                                uint64_t *data_section_start_offset_out) {
  if (dctx_filepath == NULL || root_node_out == NULL) {
    log_error(
        "dctx_reader: Invalid arguments (filepath or root_node_out is NULL).");
    return false;
  }
  *root_node_out = NULL; // Initialize output

  FILE *fp = fopen(dctx_filepath, "rb");
  if (fp == NULL) {
    log_error("dctx_reader: Failed to open .dircontxt file '%s': %s",
              dctx_filepath, strerror(errno));
    return false;
  }

  bool success = false; // Assume failure until all steps complete

  // 1. Read and Verify Signature
  char signature_buf[DIRCONTXT_SIGNATURE_LEN + 1]; // +1 for safety null term
  if (fread(signature_buf, 1, DIRCONTXT_SIGNATURE_LEN, fp) !=
      DIRCONTXT_SIGNATURE_LEN) {
    log_error("dctx_reader: Failed to read file signature from '%s'.",
              dctx_filepath);
    goto cleanup;
  }
  signature_buf[DIRCONTXT_SIGNATURE_LEN] = '\0';
  if (strcmp(signature_buf, DIRCONTXT_FILE_SIGNATURE) != 0) {
    log_error(
        "dctx_reader: Invalid file signature in '%s'. Expected '%s', got '%s'.",
        dctx_filepath, DIRCONTXT_FILE_SIGNATURE, signature_buf);
    goto cleanup;
  }
  log_debug("dctx_reader: File signature verified.");

  // 2. Read the Root Node's metadata
  //    The first node after signature is always the root.
  DirContextTreeNode *root = read_single_node_metadata(fp);
  if (root == NULL) {
    log_error("dctx_reader: Failed to read root node metadata from '%s'.",
              dctx_filepath);
    goto cleanup;
  }
  if (root->type != NODE_TYPE_DIRECTORY) {
    log_error("dctx_reader: Root node in '%s' is not a directory (type: %d). "
              "Corrupted file.",
              dctx_filepath, root->type);
    free_tree_recursive(root); // Free the incorrectly typed root
    goto cleanup;
  }

  // 3. Recursively Read Children for the Root Node
  if (root->num_children > 0) {
    if (!read_children_for_directory_node(fp, root)) {
      log_error("dctx_reader: Failed to read children for root node in '%s'.",
                dctx_filepath);
      free_tree_recursive(root); // Free partially built tree
      goto cleanup;
    }
  }

  // If successful so far, the entire tree structure (header) has been read.
  // The current file pointer position in 'fp' is now at the start of the Data
  // Section.
  if (data_section_start_offset_out != NULL) {
    long current_pos = ftell(fp);
    if (current_pos == -1L) {
      log_error("dctx_reader: ftell failed after reading header: %s",
                strerror(errno));
      free_tree_recursive(root);
      goto cleanup;
    }
    *data_section_start_offset_out = (uint64_t)current_pos;
    log_debug("dctx_reader: Data section starts at offset %llu.",
              (unsigned long long)*data_section_start_offset_out);
  }

  *root_node_out = root;
  success = true;
  log_info("dctx_reader: Successfully parsed header of '%s'.", dctx_filepath);

cleanup:
  if (fp != NULL) {
    fclose(fp);
  }
  if (!success &&
      *root_node_out != NULL) { // If we set root_node_out but then failed
    free_tree_recursive(*root_node_out);
    *root_node_out = NULL;
  }
  return success;
}

bool dctx_read_file_content(FILE *dctx_fp,
                            uint64_t data_section_start_offset_in_file,
                            const DirContextTreeNode *file_node_info,
                            char *buffer_out, size_t buffer_size) {
  if (dctx_fp == NULL || file_node_info == NULL || buffer_out == NULL) {
    log_error("dctx_read_file_content: Invalid arguments.");
    return false;
  }
  if (file_node_info->type != NODE_TYPE_FILE) {
    log_error("dctx_read_file_content: Node '%s' is not a file.",
              file_node_info->relative_path);
    return false;
  }
  if (buffer_size < file_node_info->content_size) {
    log_error("dctx_read_file_content: Buffer too small for file '%s' (need "
              "%llu, got %zu).",
              file_node_info->relative_path,
              (unsigned long long)file_node_info->content_size, buffer_size);
    return false;
  }

  uint64_t absolute_file_offset =
      data_section_start_offset_in_file +
      file_node_info->content_offset_in_data_section;

  if (fseek(dctx_fp, (long)absolute_file_offset, SEEK_SET) != 0) {
    log_error("dctx_read_file_content: Failed to seek to offset %llu for file "
              "'%s': %s",
              (unsigned long long)absolute_file_offset,
              file_node_info->relative_path, strerror(errno));
    return false;
  }

  size_t bytes_read =
      fread(buffer_out, 1, file_node_info->content_size, dctx_fp);
  if (bytes_read != file_node_info->content_size) {
    log_error("dctx_read_file_content: Failed to read content for file '%s'. "
              "Expected %llu bytes, got %zu. Error: %s",
              file_node_info->relative_path,
              (unsigned long long)file_node_info->content_size, bytes_read,
              feof(dctx_fp) ? "EOF" : strerror(errno));
    return false;
  }

  log_debug(
      "dctx_read_file_content: Successfully read %llu bytes for file '%s'.",
      (unsigned long long)file_node_info->content_size,
      file_node_info->relative_path);
  return true;
}

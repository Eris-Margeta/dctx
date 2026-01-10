/* src/utils.c */
#include "utils.h"
#include "platform.h" // For PLATFORM_DIR_SEPARATOR

#include <errno.h>  // For errno, perror
#include <stdarg.h> // For va_list, va_start, va_end
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

// --- String Utilities ---

char *safe_strncpy(char *dest, const char *src, size_t n) {
  if (n == 0)
    return dest;
  strncpy(dest, src, n - 1);
  dest[n - 1] = '\0'; // Ensure null termination
  return dest;
}

void trim_trailing_newline(char *str) {
  if (str == NULL)
    return;
  size_t len = strlen(str);
  while (len > 0 && (str[len - 1] == '\n' || str[len - 1] == '\r')) {
    str[--len] = '\0';
  }
}

// --- File I/O Utilities ---

char *read_line_from_file(FILE *fp) {
  if (fp == NULL)
    return NULL;

  size_t buffer_size = 128;
  char *buffer = (char *)malloc(buffer_size);
  if (buffer == NULL) {
    perror("read_line_from_file: malloc failed for buffer");
    return NULL;
  }

  size_t current_pos = 0;
  int c;

  while ((c = fgetc(fp)) != EOF && c != '\n') {
    if (current_pos >= buffer_size - 1) {
      buffer_size *= 2;
      char *new_buffer = (char *)realloc(buffer, buffer_size);
      if (new_buffer == NULL) {
        perror("read_line_from_file: realloc failed for buffer");
        free(buffer);
        return NULL;
      }
      buffer = new_buffer;
    }
    buffer[current_pos++] = (char)c;
  }

  buffer[current_pos] = '\0';

  if (c == EOF && current_pos == 0) {
    free(buffer);
    return NULL;
  }

  return buffer;
}

// --- Error Handling ---
#ifndef NDEBUG
#define DEBUG_LOGGING_ENABLED 1
#else
#define DEBUG_LOGGING_ENABLED 0
#endif

void log_error(const char *message_format, ...) {
  fprintf(stderr, "[ERROR] ");
  va_list args;
  va_start(args, message_format);
  vfprintf(stderr, message_format, args);
  va_end(args);
  fprintf(stderr, "\n");
}

void log_info(const char *message_format, ...) {
  printf("[INFO] ");
  va_list args;
  va_start(args, message_format);
  vprintf(message_format, args);
  va_end(args);
  printf("\n");
}

void log_debug(const char *message_format, ...) {
  if (DEBUG_LOGGING_ENABLED) {
    printf("[DEBUG] ");
    va_list args;
    va_start(args, message_format);
    vprintf(message_format, args);
    va_end(args);
    printf("\n");
  }
}

// --- Tree Utilities ---

void free_tree_recursive(DirContextTreeNode *node) {
  if (node == NULL) {
    return;
  }
  if (node->type == NODE_TYPE_DIRECTORY) {
    for (uint32_t i = 0; i < node->num_children; ++i) {
      free_tree_recursive(node->children[i]);
    }
    free(node->children);
  }
  free(node);
}

DirContextTreeNode *create_node(NodeType type,
                                const char *relative_path_in_archive,
                                const char *disk_path_for_stat) {
  DirContextTreeNode *node =
      (DirContextTreeNode *)malloc(sizeof(DirContextTreeNode));
  if (node == NULL) {
    perror("create_node: malloc failed");
    return NULL;
  }

  node->type = type;
  safe_strncpy(node->relative_path, relative_path_in_archive, MAX_PATH_LEN);
  safe_strncpy(node->disk_path, disk_path_for_stat, MAX_PATH_LEN);

  node->content_offset_in_data_section = 0;
  node->content_size = 0; // Default initialization

  struct stat stat_buf;
  if (platform_get_file_stat(disk_path_for_stat, &stat_buf) == 0) {
    node->last_modified_timestamp = platform_get_mod_time(&stat_buf);

    // FIX: Populate content_size from the file system stat
    if (node->type == NODE_TYPE_FILE) {
      node->content_size = (uint64_t)stat_buf.st_size;
    }
  } else {
    log_error("Failed to stat %s, setting timestamp to 0.", disk_path_for_stat);
    node->last_modified_timestamp = 0;
  }

  node->children = NULL;
  node->num_children = 0;
  node->children_capacity = 0;

  node->generated_id_for_llm[0] = '\0';

  if (type == NODE_TYPE_DIRECTORY) {
    node->children_capacity = 4;
    node->children = (DirContextTreeNode **)malloc(
        node->children_capacity * sizeof(DirContextTreeNode *));
    if (node->children == NULL) {
      perror("create_node: malloc failed for children array");
      free(node);
      return NULL;
    }
  }
  return node;
}

bool add_child_to_parent_node(DirContextTreeNode *parent,
                              DirContextTreeNode *child) {
  if (parent == NULL || parent->type != NODE_TYPE_DIRECTORY || child == NULL) {
    return false;
  }

  if (parent->num_children >= parent->children_capacity) {
    uint32_t new_capacity =
        (parent->children_capacity == 0) ? 4 : parent->children_capacity * 2;
    DirContextTreeNode **new_children = (DirContextTreeNode **)realloc(
        parent->children, new_capacity * sizeof(DirContextTreeNode *));
    if (new_children == NULL) {
      perror("add_child_to_parent_node: realloc failed");
      return false;
    }
    parent->children = new_children;
    parent->children_capacity = new_capacity;
  }

  parent->children[parent->num_children++] = child;
  return true;
}

char *get_directory_basename(const char *path) {
  if (path == NULL || path[0] == '\0') {
    return strdup(".");
  }

  char temp_path[MAX_PATH_LEN];
  safe_strncpy(temp_path, path, MAX_PATH_LEN);

  size_t len = strlen(temp_path);
  while (len > 0 && temp_path[len - 1] == PLATFORM_DIR_SEPARATOR) {
    temp_path[--len] = '\0';
  }

  if (len == 0 && path[0] == PLATFORM_DIR_SEPARATOR) {
    return strdup(PLATFORM_DIR_SEPARATOR_STR);
  }
  if (len == 0) {
    return strdup(".");
  }

  const char *basename_ptr = platform_get_basename(temp_path);
  if (basename_ptr == NULL) {
    return strdup(".");
  }
  return strdup(basename_ptr);
}

void print_tree_recursive(const DirContextTreeNode *node, int indent_level) {
  if (node == NULL)
    return;

  for (int i = 0; i < indent_level; ++i) {
    printf("  ");
  }

  if (node->type == NODE_TYPE_DIRECTORY) {
    printf("[%s/] (mod: %lld, children: %u, id_llm: %s)\n", node->relative_path,
           (long long)node->last_modified_timestamp, node->num_children,
           node->generated_id_for_llm[0] == '\0' ? "(none)"
                                                 : node->generated_id_for_llm);
    for (uint32_t i = 0; i < node->num_children; ++i) {
      print_tree_recursive(node->children[i], indent_level + 1);
    }
  } else { // NODE_TYPE_FILE
    printf("%s (mod: %lld, offset: %llu, size: %llu, id_llm: %s)\n",
           node->relative_path, (long long)node->last_modified_timestamp,
           (unsigned long long)node->content_offset_in_data_section,
           (unsigned long long)node->content_size,
           node->generated_id_for_llm[0] == '\0' ? "(none)"
                                                 : node->generated_id_for_llm);
  }
}

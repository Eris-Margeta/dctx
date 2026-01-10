#define _GNU_SOURCE // For D_TYPE in dirent on some Linux systems, generally
                    // good for compatibility
#include "walker.h"
#include "ignore.h" // For should_ignore_item
#include "platform.h" // For platform_get_file_stat, platform_is_dir, platform_join_paths, etc.
#include "utils.h" // For create_node, add_child_to_parent_node, log_debug, log_error

#include <dirent.h> // For opendir, readdir, closedir
#include <errno.h>  // For errno
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

// Internal recursive helper function for walk_directory_and_build_tree
static bool walk_recursive_helper(
    DirContextTreeNode *current_parent_node,
    const char *current_parent_disk_path, // Absolute path of
                                          // current_parent_node on disk
    const char
        *base_target_disk_path, // Absolute path of the initial target directory
    const IgnoreRule *ignore_rules, int ignore_rule_count,
    int *processed_item_count_out) {
  DIR *dir_stream = opendir(current_parent_disk_path);
  if (dir_stream == NULL) {
    log_error("Failed to open directory %s: %s", current_parent_disk_path,
              strerror(errno));
    return false; // Cannot proceed with this directory
  }

  log_debug("Walking directory: %s (relative in archive: '%s')",
            current_parent_disk_path, current_parent_node->relative_path);

  struct dirent *entry;
  errno = 0; // Reset errno before starting loop
  while ((entry = readdir(dir_stream)) != NULL) {
    const char *entry_name = entry->d_name;

    // Skip "." and ".." entries
    if (strcmp(entry_name, ".") == 0 || strcmp(entry_name, "..") == 0) {
      continue;
    }

    char child_disk_path[MAX_PATH_LEN];
    if (!platform_join_paths(current_parent_disk_path, entry_name,
                             child_disk_path, MAX_PATH_LEN)) {
      log_error("Failed to construct child disk path for %s in %s", entry_name,
                current_parent_disk_path);
      continue; // Skip this entry
    }

    char child_relative_path_in_archive[MAX_PATH_LEN];
    if (strlen(current_parent_node->relative_path) == 0 ||
        (strlen(current_parent_node->relative_path) == 1 &&
         current_parent_node->relative_path[0] == '.')) {
      safe_strncpy(child_relative_path_in_archive, entry_name, MAX_PATH_LEN);
    } else {
      if (!platform_join_paths(current_parent_node->relative_path, entry_name,
                               child_relative_path_in_archive, MAX_PATH_LEN)) {
        log_error("Failed to construct child relative path for %s under %s",
                  entry_name, current_parent_node->relative_path);
        continue; // Skip this entry
      }
    }

    struct stat stat_buf;
    if (platform_get_file_stat(child_disk_path, &stat_buf) != 0) {
      log_error("Failed to stat %s: %s. Skipping.", child_disk_path,
                strerror(errno));
      continue;
    }

    bool is_child_dir = platform_is_dir(&stat_buf);
    bool is_child_file = platform_is_reg_file(&stat_buf);

    if (!is_child_dir && !is_child_file) {
      log_debug("Skipping non-file/non-directory item: %s", child_disk_path);
      continue; // Skip sockets, pipes, etc.
    }

    // Prepare path for should_ignore_item (needs trailing slash for dirs if
    // rules expect it)
    char effective_relative_path_for_ignore[MAX_PATH_LEN];
    safe_strncpy(effective_relative_path_for_ignore,
                 child_relative_path_in_archive, MAX_PATH_LEN);
    if (is_child_dir) {
      size_t len = strlen(effective_relative_path_for_ignore);
      if (len > 0 &&
          effective_relative_path_for_ignore[len - 1] !=
              PLATFORM_DIR_SEPARATOR &&
          len < MAX_PATH_LEN - 1) {
        effective_relative_path_for_ignore[len] = PLATFORM_DIR_SEPARATOR;
        effective_relative_path_for_ignore[len + 1] = '\0';
      }
    }

    if (should_ignore_item(effective_relative_path_for_ignore, entry_name,
                           is_child_dir, ignore_rules, ignore_rule_count)) {
      log_debug("Ignoring: %s (relative: %s)", child_disk_path,
                child_relative_path_in_archive);
      continue;
    }

    log_debug("Processing: %s (relative: %s)", child_disk_path,
              child_relative_path_in_archive);
    if (processed_item_count_out) {
      (*processed_item_count_out)++;
    }

    NodeType node_type = is_child_dir ? NODE_TYPE_DIRECTORY : NODE_TYPE_FILE;
    DirContextTreeNode *child_node =
        create_node(node_type, child_relative_path_in_archive, child_disk_path);
    if (child_node == NULL) {
      log_error("Failed to create tree node for %s. Skipping.",
                child_disk_path);
      continue; // Critical error creating node
    }

    if (!add_child_to_parent_node(current_parent_node, child_node)) {
      log_error("Failed to add child node %s to parent %s. Skipping.",
                child_disk_path, current_parent_disk_path);
      free_tree_recursive(child_node); // Clean up unattached child
      continue;
    }

    if (is_child_dir) {
      // Recursively walk the subdirectory
      if (!walk_recursive_helper(child_node, child_disk_path,
                                 base_target_disk_path, ignore_rules,
                                 ignore_rule_count, processed_item_count_out)) {
        // Error occurred in subdirectory, but we can continue with other
        // siblings
        log_debug("Error walking subdirectory %s, but continuing.",
                  child_disk_path);
      }
    }
  } // end while readdir

  if (errno != 0) { // Check if readdir loop terminated due to an error
    log_error("Error reading directory %s: %s", current_parent_disk_path,
              strerror(errno));
  }

  closedir(dir_stream);
  return true; // Successfully walked this directory (or handled errors within
               // it)
}

DirContextTreeNode *walk_directory_and_build_tree(
    const char *target_dir_path_on_disk, // This is absolute
    const IgnoreRule *ignore_rules, int ignore_rule_count,
    int *processed_item_count_out) {
  if (target_dir_path_on_disk == NULL) {
    log_error("Target directory path is NULL.");
    return NULL;
  }
  if (processed_item_count_out) {
    *processed_item_count_out = 0;
  }

  struct stat stat_buf;
  if (platform_get_file_stat(target_dir_path_on_disk, &stat_buf) != 0) {
    log_error("Failed to stat target directory %s: %s", target_dir_path_on_disk,
              strerror(errno));
    return NULL;
  }
  if (!platform_is_dir(&stat_buf)) {
    log_error("Target path %s is not a directory.", target_dir_path_on_disk);
    return NULL;
  }

  // The root node's relative path in the archive is effectively "." or empty
  // string, representing the base of the walked directory.
  DirContextTreeNode *root_node =
      create_node(NODE_TYPE_DIRECTORY, "", target_dir_path_on_disk);
  if (root_node == NULL) {
    log_error("Failed to create root node for directory %s.",
              target_dir_path_on_disk);
    return NULL;
  }

  if (processed_item_count_out) {
    (*processed_item_count_out)++;
  }

  log_info("Starting directory walk from: %s", target_dir_path_on_disk);

  if (!walk_recursive_helper(root_node, target_dir_path_on_disk,
                             target_dir_path_on_disk, ignore_rules,
                             ignore_rule_count, processed_item_count_out)) {
    log_error("Initial directory walk failed for %s.", target_dir_path_on_disk);
    free_tree_recursive(root_node);
    return NULL;
  }

  log_info("Directory walk completed. Processed %d items (files/dirs).",
           (processed_item_count_out ? *processed_item_count_out : 0));
  return root_node;
}

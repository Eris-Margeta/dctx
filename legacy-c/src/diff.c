#include "diff.h"
#include "utils.h" // For safe_strncpy and logging
#include <stdlib.h>
#include <string.h>

// --- Static Helper Function Declarations ---

// Creates and initializes an empty DiffReport.
static DiffReport *create_diff_report();

// Adds a new change entry to a DiffReport, handling dynamic array resizing.
static void add_change_to_report(DiffReport *report, ChangeType type,
                                 const DirContextTreeNode *node);

// Recursively compares two directory nodes and populates the diff report.
static void compare_nodes_recursive(const DirContextTreeNode *old_node,
                                    const DirContextTreeNode *new_node,
                                    DiffReport *report);

// Finds a child node with a matching relative_path within a parent's children
// list.
static const DirContextTreeNode *
find_child_node(const DirContextTreeNode *parent, const char *relative_path);

// --- Public Function Implementations ---

DiffReport *compare_trees(const DirContextTreeNode *old_root,
                          const DirContextTreeNode *new_root) {
  DiffReport *report = create_diff_report();
  if (!report) {
    return NULL;
  }

  if (old_root == NULL && new_root != NULL) {
    add_change_to_report(report, ITEM_ADDED, new_root);
  } else if (old_root != NULL && new_root == NULL) {
    add_change_to_report(report, ITEM_REMOVED, old_root);
  } else if (old_root != NULL && new_root != NULL) {
    compare_nodes_recursive(old_root, new_root, report);
  }

  return report;
}

void free_diff_report(DiffReport *report) {
  if (report != NULL) {
    free(report->entries);
    free(report);
  }
}

// --- Static Helper Function Implementations ---

static DiffReport *create_diff_report() {
  DiffReport *report = (DiffReport *)malloc(sizeof(DiffReport));
  if (report == NULL) {
    log_error("Failed to allocate memory for DiffReport.");
    return NULL;
  }
  report->has_changes = false;
  report->count = 0;
  report->capacity = 16; // Initial capacity
  report->entries = (DiffEntry *)malloc(report->capacity * sizeof(DiffEntry));
  if (report->entries == NULL) {
    log_error("Failed to allocate memory for DiffReport entries.");
    free(report);
    return NULL;
  }
  return report;
}

static void add_change_to_report(DiffReport *report, ChangeType type,
                                 const DirContextTreeNode *node) {
  if (report == NULL || node == NULL)
    return;

  report->has_changes = true;

  // Resize the entries array if capacity is reached
  if (report->count >= report->capacity) {
    report->capacity *= 2;
    DiffEntry *new_entries = (DiffEntry *)realloc(
        report->entries, report->capacity * sizeof(DiffEntry));
    if (new_entries == NULL) {
      log_error("Failed to reallocate memory for DiffReport entries.");
      // We lose this entry, but the program can continue.
      return;
    }
    report->entries = new_entries;
  }

  // Populate the new entry
  DiffEntry *entry = &report->entries[report->count];
  entry->type = type;
  entry->node_type = node->type;
  safe_strncpy(entry->relative_path, node->relative_path, MAX_PATH_LEN);

  report->count++;
}

static const DirContextTreeNode *
find_child_node(const DirContextTreeNode *parent, const char *relative_path) {
  if (parent == NULL || parent->type != NODE_TYPE_DIRECTORY) {
    return NULL;
  }
  for (uint32_t i = 0; i < parent->num_children; ++i) {
    if (strcmp(parent->children[i]->relative_path, relative_path) == 0) {
      return parent->children[i];
    }
  }
  return NULL;
}

static void compare_nodes_recursive(const DirContextTreeNode *old_node,
                                    const DirContextTreeNode *new_node,
                                    DiffReport *report) {
  // --- Pass 1: Check for additions and modifications ---
  // Iterate through all items in the NEW directory.
  for (uint32_t i = 0; i < new_node->num_children; ++i) {
    DirContextTreeNode *new_child = new_node->children[i];
    const DirContextTreeNode *old_child =
        find_child_node(old_node, new_child->relative_path);

    if (old_child == NULL) {
      // Item exists in new tree but not in old tree: ADDED
      add_change_to_report(report, ITEM_ADDED, new_child);
    } else {
      // Item exists in both trees: Check for modification.
      bool is_modified = false;
      if (new_child->type != old_child->type) {
        is_modified = true; // Type changed (e.g., file became a dir)
      } else if (new_child->type == NODE_TYPE_FILE) {
        if (new_child->content_size != old_child->content_size ||
            new_child->last_modified_timestamp !=
                old_child->last_modified_timestamp) {
          is_modified = true;
        }
      } else { // It's a directory
        if (new_child->last_modified_timestamp !=
            old_child->last_modified_timestamp) {
          // For directories, we'll rely on recursive calls to find inner
          // changes, but a timestamp change on the dir itself can also be a
          // valid signal. For now, let's just recurse.
        }
      }

      if (is_modified) {
        add_change_to_report(report, ITEM_MODIFIED, new_child);
      }

      // If both are directories, we need to go deeper.
      if (new_child->type == NODE_TYPE_DIRECTORY &&
          old_child->type == NODE_TYPE_DIRECTORY) {
        compare_nodes_recursive(old_child, new_child, report);
      }
    }
  }

  // --- Pass 2: Check for removals ---
  // Iterate through all items in the OLD directory.
  for (uint32_t i = 0; i < old_node->num_children; ++i) {
    DirContextTreeNode *old_child = old_node->children[i];
    const DirContextTreeNode *new_child =
        find_child_node(new_node, old_child->relative_path);

    if (new_child == NULL) {
      // Item exists in old tree but not in new tree: REMOVED
      add_change_to_report(report, ITEM_REMOVED, old_child);
    }
  }
}

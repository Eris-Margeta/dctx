#ifndef DIFF_H
#define DIFF_H

#include "datatypes.h"
#include <stdbool.h>

// --- Data Structures for Diff Reporting ---

// Enum to describe the type of change for a file or directory.
typedef enum { ITEM_ADDED, ITEM_REMOVED, ITEM_MODIFIED } ChangeType;

// A struct to represent a single change between two tree states.
typedef struct {
  ChangeType type;
  NodeType node_type;
  char relative_path[MAX_PATH_LEN];
  // For MODIFIED items, we can store old metadata if needed, but for now
  // the path is enough to look up the node in the new tree.
} DiffEntry;

// A struct to hold the complete comparison report.
typedef struct {
  bool has_changes;
  DiffEntry *entries;
  int count;
  int capacity;
} DiffReport;

// --- Public Functions ---

// Compares two directory trees (an old state and a new state) and returns a
// report detailing all additions, removals, and modifications.
//
// The comparison logic is as follows:
// - ADDED: An item exists in the new_tree but not the old_tree.
// - REMOVED: An item exists in the old_tree but not the new_tree.
// - MODIFIED: An item exists in both, but its modification timestamp or size
//             (for files) has changed.
//
// Parameters:
//   old_root: The root node of the previously saved directory tree.
//   new_root: The root node of the freshly scanned directory tree.
//
// Returns:
//   A pointer to a dynamically allocated DiffReport. The caller is responsible
//   for freeing this report using free_diff_report().
DiffReport *compare_trees(const DirContextTreeNode *old_root,
                          const DirContextTreeNode *new_root);

// Frees all memory associated with a DiffReport struct.
void free_diff_report(DiffReport *report);

#endif // DIFF_H

#ifndef WALKER_H
#define WALKER_H

#include "datatypes.h" // For DirContextTreeNode, IgnoreRule
#include <stdbool.h>

// --- Core Directory Walking Function ---

// Walks the specified directory recursively, building a tree of
// DirContextTreeNode. It respects the ignore rules provided.
//
// Parameters:
//   target_dir_path: Absolute path of the directory to start walking from.
//   ignore_rules: Array of loaded IgnoreRule structs.
//   ignore_rule_count: Number of rules in the ignore_rules array.
//   processed_item_count_out: (Optional) Pointer to an int to store the total
//   number of files and directories processed (not ignored).
//
// Returns:
//   A pointer to the root DirContextTreeNode of the generated tree.
//   The root node itself represents the target_dir_path.
//   Returns NULL on critical error (e.g., target_dir_path is not a directory or
//   unreadable). The caller is responsible for freeing the returned tree using
//   free_tree_recursive().
DirContextTreeNode *walk_directory_and_build_tree(
    const char *target_dir_path, const IgnoreRule *ignore_rules,
    int ignore_rule_count, int *processed_item_count_out);

#endif // WALKER_H

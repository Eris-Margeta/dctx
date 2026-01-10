#ifndef UTILS_H
#define UTILS_H

#include "datatypes.h" // For DirContextTreeNode
#include <stdbool.h>   // For bool
#include <stdio.h>     // For FILE*

// --- String Utilities ---

// Safely copy a string, ensuring null termination.
// Similar to strncpy but guarantees null termination if n > 0.
// Returns dest.
char *safe_strncpy(char *dest, const char *src, size_t n);

// Trim trailing newline characters (LF or CRLF) from a string in-place.
void trim_trailing_newline(char *str);

// --- File I/O Utilities ---

// Read an entire line from a file stream, dynamically allocating memory.
// The caller is responsible for freeing the returned string.
// Returns NULL on EOF or error.
char *read_line_from_file(FILE *fp);

// --- Error Handling ---
// Basic error reporting, can be expanded.
void log_error(const char *message_format, ...);
void log_info(const char *message_format, ...);
void log_debug(const char *message_format, ...); // Controlled by a DEBUG flag

// --- Tree Utilities ---

// Recursively free the memory allocated for a DirContextTreeNode and its
// children.
void free_tree_recursive(DirContextTreeNode *node);

// Create a new tree node.
// `disk_path_for_stat` is the path used to stat the file/dir to get its mod
// time and type. `relative_path_in_archive` is the path that will be stored in
// the .dircontxt file.
DirContextTreeNode *create_node(NodeType type,
                                const char *relative_path_in_archive,
                                const char *disk_path_for_stat);

// Add a child node to a parent node's children list (handles dynamic array).
bool add_child_to_parent_node(DirContextTreeNode *parent,
                              DirContextTreeNode *child);

// Get the base name of a directory (e.g., "myfolder" from "/path/to/myfolder/"
// or "/path/to/myfolder") The caller is responsible for freeing the returned
// string.
char *get_directory_basename(const char *path);

// For debug printing of the tree structure
void print_tree_recursive(const DirContextTreeNode *node, int indent_level);

#endif // UTILS_H

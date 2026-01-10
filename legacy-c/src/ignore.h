#ifndef IGNORE_H
#define IGNORE_H

#include "datatypes.h" // For IgnoreRule, MAX_PATH_LEN
#include <stdbool.h>

#define DEFAULT_IGNORE_FILENAME ".dircontxtignore"

// --- Core Ignore List Functions ---

// MODIFIED: Updated documentation to reflect the new ignore hierarchy.
// Loads ignore rules from three sources in a specific order of precedence:
// 1. Hardcoded Default Rules: A built-in list of common ignores (e.g., .git/).
// 2. Global Ignore File: Rules from a user-wide file
// (~/.config/dircontxt/ignore).
// 3. Project Ignore File: Rules from .dircontxtignore in the target directory.
// Rules loaded later override rules loaded earlier if they match the same file.
//
// Parameters:
//   base_dir_path: Absolute path to the target directory.
//   output_filename_to_ignore: The name of the .dircontxt file being generated,
//                              which will also be ignored.
//   rules_array_out: Pointer to an array of IgnoreRule structs that will be
//                    allocated and filled.
//   rule_count_out: Pointer to an integer to store the total number of rules
//   loaded.
//
// Returns:
//   True if successful, false on a critical error (like memory allocation
//   failure).
bool load_ignore_rules(const char *base_dir_path,
                       const char *output_filename_to_ignore,
                       IgnoreRule **rules_array_out, int *rule_count_out);

// Checks if a given item (file or directory) should be ignored based on the
// full list of loaded rules. The logic is based on "last matching rule wins",
// allowing negation patterns (!) to work correctly.
//
// Parameters:
//   item_relative_path: Path of the item relative to the dircontxt root (e.g.,
//   "src/file.c"). item_name: Just the name of the item (e.g., "file.c").
//   is_item_dir: True if the item is a directory, false if it's a file.
//   rules: Array of loaded IgnoreRule structs.
//   rule_count: Number of rules in the array.
//
// Returns:
//   True if the item should be ignored, false otherwise.
bool should_ignore_item(const char *item_relative_path, const char *item_name,
                        bool is_item_dir, const IgnoreRule *rules,
                        int rule_count);

// Frees the memory allocated for the ignore rules array.
void free_ignore_rules_array(IgnoreRule *rules_array, int rule_count);

// Parses a single line from an ignore file into an IgnoreRule struct.
// This function understands advanced syntax like negation ('!'), directory
// markers ('/'), and wildcards ('*').
bool parse_ignore_pattern_line(const char *line, IgnoreRule *rule_out);

#endif // IGNORE_H

#include "ignore.h"
// FIX 1: Added datatypes.h to resolve the 'NodeType' and 'IgnoreRule'
// definitions before they are used in other included headers.
#include "datatypes.h"
#include "platform.h" // For platform_join_paths, PLATFORM_DIR_SEPARATOR_STR, PLATFORM_DIR_SEPARATOR
#include "utils.h" // For log_debug, log_info, log_error, read_line_from_file, trim_trailing_newline, safe_strncpy

#include <ctype.h> // For isspace
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

// --- Helper Functions ---

// Helper to add a rule to the dynamic array of rules.
static bool add_rule_to_list(IgnoreRule rule, IgnoreRule **rules_array_out,
                             int *rule_count_out, int *capacity_out) {
  if (*rule_count_out >= *capacity_out) {
    int new_capacity = (*capacity_out == 0) ? 16 : *capacity_out * 2;
    IgnoreRule *new_array = (IgnoreRule *)realloc(
        *rules_array_out, new_capacity * sizeof(IgnoreRule));
    if (new_array == NULL) {
      perror("add_rule_to_list: realloc failed");
      return false;
    }
    *rules_array_out = new_array;
    *capacity_out = new_capacity;
  }
  (*rules_array_out)[*rule_count_out] = rule; // Struct copy
  (*rule_count_out)++;
  return true;
}

// Helper to load rules from a specific file path into the rules list.
static bool load_rules_from_file(const char *filepath,
                                 IgnoreRule **rules_array_out,
                                 int *rule_count_out, int *capacity_out) {
  FILE *fp = fopen(filepath, "r");
  if (fp == NULL) {
    if (errno != ENOENT) { // Report error only if it's not "File Not Found"
      log_info("Could not read ignore file %s: %s.", filepath, strerror(errno));
    }
    return true; // It's not an error for an ignore file to be missing.
  }

  log_info("Loading ignore rules from: %s", filepath);
  char *line;
  while ((line = read_line_from_file(fp)) != NULL) {
    IgnoreRule rule;
    if (parse_ignore_pattern_line(line, &rule)) {
      // FIX 2: Removed the extra '&' from capacity_out. It is already a
      // pointer.
      if (!add_rule_to_list(rule, rules_array_out, rule_count_out,
                            capacity_out)) {
        free(line);
        fclose(fp);
        return false;
      }
    }
    free(line);
  }
  fclose(fp);
  return true;
}

// --- Public Function Implementations ---

// MODIFIED: Rewritten to parse the new, more advanced syntax.
bool parse_ignore_pattern_line(const char *orig_line, IgnoreRule *rule_out) {
  if (orig_line == NULL || rule_out == NULL)
    return false;

  char line_buffer[MAX_PATH_LEN];
  safe_strncpy(line_buffer, orig_line, MAX_PATH_LEN);
  trim_trailing_newline(line_buffer);

  const char *line = line_buffer;
  // Trim leading whitespace
  while (isspace((unsigned char)*line))
    line++;

  // Handle comments and empty lines
  if (line[0] == '\0' || line[0] == '#') {
    return false;
  }

  memset(rule_out, 0, sizeof(IgnoreRule));
  rule_out->type = PATTERN_TYPE_INVALID; // Default to invalid

  // Check for negation
  if (line[0] == '!') {
    rule_out->is_negation = true;
    line++; // Advance past the '!'
  }

  safe_strncpy(rule_out->pattern, line, MAX_PATH_LEN);
  size_t len = strlen(rule_out->pattern);

  // Check if it's a directory-only pattern
  if (len > 0 && rule_out->pattern[len - 1] == PLATFORM_DIR_SEPARATOR) {
    rule_out->is_dir_only = true;
    rule_out->pattern[len - 1] = '\0'; // Remove trailing slash
    len--;
  }

  // Determine pattern type
  if (strchr(rule_out->pattern, PLATFORM_DIR_SEPARATOR) != NULL) {
    // Contains a '/', so it's a path-based match
    rule_out->type = PATTERN_TYPE_PATH;
    if (len > 0 && rule_out->pattern[len - 1] == '*') {
      rule_out->type = PATTERN_TYPE_PREFIX;
      rule_out->pattern[len - 1] = '\0'; // remove the '*'
    }
  } else if (len > 0 && rule_out->pattern[0] == '*') {
    // Starts with '*', so it's a suffix match (e.g., "*.log")
    rule_out->type = PATTERN_TYPE_SUFFIX;
    // Shift pattern to the left to remove '*'
    memmove(rule_out->pattern, rule_out->pattern + 1, len);
  } else if (len > 0) {
    // No '/' and doesn't start with '*', so it's a basename match
    rule_out->type = PATTERN_TYPE_BASENAME;
  }

  // A pattern is only valid if we successfully assigned a type
  return rule_out->type != PATTERN_TYPE_INVALID;
}

// MODIFIED: Rewritten to load from default, global, and project sources.
bool load_ignore_rules(const char *base_dir_path,
                       const char *output_filename_to_ignore,
                       IgnoreRule **rules_array_out, int *rule_count_out) {
  *rules_array_out = NULL;
  *rule_count_out = 0;
  int capacity = 0;

  // --- 1. Add Hardcoded Default Rules (Lowest Priority) ---
  const char *default_patterns[] = {".git/", ".DS_Store", "node_modules/"};
  for (size_t i = 0; i < sizeof(default_patterns) / sizeof(default_patterns[0]);
       i++) {
    IgnoreRule rule;
    if (parse_ignore_pattern_line(default_patterns[i], &rule)) {
      add_rule_to_list(rule, rules_array_out, rule_count_out, &capacity);
    }
  }
  // Also ignore the output file itself
  if (output_filename_to_ignore && output_filename_to_ignore[0] != '\0') {
    IgnoreRule rule;
    if (parse_ignore_pattern_line(output_filename_to_ignore, &rule)) {
      add_rule_to_list(rule, rules_array_out, rule_count_out, &capacity);
    }
  }

  // --- 2. Load Global Ignore File (Medium Priority) ---
  const char *home_dir = getenv("HOME");
  if (home_dir) {
    char global_ignore_path[MAX_PATH_LEN];
    snprintf(global_ignore_path, MAX_PATH_LEN, "%s/.config/dircontxt/ignore",
             home_dir);
    if (!load_rules_from_file(global_ignore_path, rules_array_out,
                              rule_count_out, &capacity))
      return false; // Critical error
  }

  // --- 3. Load Project-Specific Ignore File (Highest Priority) ---
  char project_ignore_path[MAX_PATH_LEN];
  if (platform_join_paths(base_dir_path, DEFAULT_IGNORE_FILENAME,
                          project_ignore_path, MAX_PATH_LEN)) {
    if (!load_rules_from_file(project_ignore_path, rules_array_out,
                              rule_count_out, &capacity))
      return false; // Critical error
  }

  return true;
}

// MODIFIED: Rewritten to let the last matching rule win.
bool should_ignore_item(const char *item_relative_path, const char *item_name,
                        bool is_item_dir, const IgnoreRule *rules,
                        int rule_count) {
  if (rules == NULL || rule_count == 0) {
    return false;
  }

  bool is_ignored = false; // Default to not ignored

  for (int i = 0; i < rule_count; ++i) {
    const IgnoreRule *rule = &rules[i];
    bool matched = false;

    // Skip directory-only rules for files
    if (rule->is_dir_only && !is_item_dir) {
      continue;
    }

    switch (rule->type) {
    // FIX 3: Added a case for PATTERN_TYPE_INVALID to handle all enum values.
    case PATTERN_TYPE_INVALID:
      break;

    case PATTERN_TYPE_PATH:
      // Exact path match
      if (strcmp(item_relative_path, rule->pattern) == 0) {
        matched = true;
      }
      break;
    case PATTERN_TYPE_PREFIX:
      // Path prefix match
      if (strncmp(item_relative_path, rule->pattern, strlen(rule->pattern)) ==
          0) {
        matched = true;
      }
      break;
    case PATTERN_TYPE_BASENAME:
      // Match against just the file/folder name
      if (strcmp(item_name, rule->pattern) == 0) {
        matched = true;
      }
      break;

    case PATTERN_TYPE_SUFFIX: {
      // FIX 4: Added curly braces to create a scope for the declarations.
      // Match against the end of the file/folder name
      size_t item_name_len = strlen(item_name);
      size_t pattern_len = strlen(rule->pattern);
      if (item_name_len >= pattern_len &&
          strcmp(item_name + (item_name_len - pattern_len), rule->pattern) ==
              0) {
        matched = true;
      }
      break;
    }
    }

    if (matched) {
      // The last rule that matches determines the outcome.
      is_ignored = !rule->is_negation;
    }
  }

  return is_ignored;
}

void free_ignore_rules_array(IgnoreRule *rules_array, int rule_count) {
  (void)rule_count;
  if (rules_array != NULL) {
    free(rules_array);
  }
}

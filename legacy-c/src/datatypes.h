#ifndef DATATYPES_H
#define DATATYPES_H

#include <limits.h> // For PATH_MAX (though we'll define our own if not available)
#include <stdbool.h> // For bool
#include <stdint.h>  // For uint64_t, uint32_t, uint16_t, uint8_t

// Define MAX_PATH_LEN if PATH_MAX is not suitably defined or to have a
// consistent buffer
#ifndef MAX_PATH_LEN
#ifdef PATH_MAX
#define MAX_PATH_LEN PATH_MAX
#else
#define MAX_PATH_LEN 4096 // A common fallback
#endif
#endif

// Forward declaration for the tree node structure
struct DirContextTreeNode;

// **************************************************************************
// FIX: Added the missing definition for NodeType.
// This must be defined before it is used inside DirContextTreeNode.
typedef enum { NODE_TYPE_FILE, NODE_TYPE_DIRECTORY } NodeType;
// **************************************************************************

// Enum to define the type of pattern match for an ignore rule.
typedef enum {
  PATTERN_TYPE_INVALID,
  PATTERN_TYPE_BASENAME, // Matches only the file/dir name (e.g.,
                         // "node_modules")
  PATTERN_TYPE_PATH,     // Matches the full relative path (e.g., "src/main.c")
  PATTERN_TYPE_SUFFIX,   // Matches a suffix wildcard (e.g., "*.log")
  PATTERN_TYPE_PREFIX,   // Matches a prefix wildcard (e.g., "build/*")
} PatternType;

// The IgnoreRule struct is now more descriptive.
typedef struct {
  char pattern[MAX_PATH_LEN];
  PatternType type;
  bool is_dir_only;
  bool is_negation; // Set to true if the pattern starts with '!'
} IgnoreRule;

// Structure for representing a file or directory in our in-memory tree
typedef struct DirContextTreeNode {
  NodeType type; // This line now works correctly.
  char relative_path[MAX_PATH_LEN];
  uint64_t last_modified_timestamp;

  // --- For files ---
  uint64_t content_offset_in_data_section;
  uint64_t content_size;
  char disk_path[MAX_PATH_LEN];

  // --- For directories ---
  struct DirContextTreeNode **children;
  uint32_t num_children;
  uint32_t children_capacity;

  // --- ADDED FOR LLM FORMATTER ID STORAGE ---
  char generated_id_for_llm[20]; // To store IDs like "F001", "D002", "ROOT"

} DirContextTreeNode;

#endif // DATATYPES_H

#ifndef WRITER_H
#define WRITER_H

#include "datatypes.h"
#include <stdbool.h>
#include <stdio.h> // For FILE* (though typically not in .h for opaque types, here for clarity)

// --- Constants for the .dircontxt format ---
// MODIFIED HERE: Signature is exactly 8 characters for content to match
// DIRCONTXT_SIGNATURE_LEN
#define DIRCONTXT_FILE_SIGNATURE "DIRCTXTV"
#define DIRCONTXT_SIGNATURE_LEN 8

// --- Core Writing Function ---

// Writes the in-memory directory tree and file contents to a .dircontxt file.
//
// Parameters:
//   output_filepath: The full path to the .dircontxt file to be
//   created/overwritten. root_node: Pointer to the root of the in-memory
//   DirContextTreeNode tree.
//              This tree should have file node content_offset_in_data_section
//              and content_size fields PRE-CALCULATED by a preliminary pass if
//              they are not calculated during this write. (Our approach will
//              calculate them during the write process).
//
// Returns:
//   True if the file was written successfully, false otherwise.
bool write_dircontxt_file(const char *output_filepath,
                          DirContextTreeNode *root_node);

#endif // WRITER_H

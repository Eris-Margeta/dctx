#ifndef LLM_FORMATTER_H
#define LLM_FORMATTER_H

#include "datatypes.h" // For DirContextTreeNode
#include "diff.h"      // For DiffReport
#include <stdbool.h>
#include <stdio.h> // For FILE*

// --- Core LLM Context File Generation Functions ---

// Generates a complete, LLM-friendly text file from a parsed .dircontxt tree
// and its binary file.
//
// Parameters:
//   llm_txt_filepath:       Path to the .llmcontext.txt file to be created.
//   root_node:              Pointer to the root of the in-memory tree.
//                           This node will be modified to store generated IDs.
//   dctx_binary_filepath:   Path to the .dircontxt binary for reading content.
//   data_section_start_offset_in_dctx_file: Byte offset where data begins.
//   version_string:         The version string (e.g., "V1.2") to write in the
//   header.
//
// Returns:
//   True if the file was generated successfully, false otherwise.
bool generate_llm_context_file(const char *llm_txt_filepath,
                               DirContextTreeNode *root_node,
                               const char *dctx_binary_filepath,
                               uint64_t data_section_start_offset_in_dctx_file,
                               const char *version_string);

// --- NEW: Stream-Based Generation Function ---

// Generates the LLM context content and writes it to an already open stream.
// This is used for writing to either a file or an in-memory buffer for the
// clipboard.
//
// Parameters are identical to generate_llm_context_file, except for the first:
//   output_stream: An open, writable FILE stream.
//
// Returns:
//   True if writing to the stream was successful, false otherwise.
bool generate_llm_context_to_stream(
    FILE *output_stream, DirContextTreeNode *root_node,
    const char *dctx_binary_filepath,
    uint64_t data_section_start_offset_in_dctx_file,
    const char *version_string);

// Generates a diff file that summarizes the changes between two versions.
//
// Parameters:
//   diff_filepath:          Path to the diff file to be created (e.g.,
//   "proj-V1.2-diff.txt"). report:                 The DiffReport containing
//   the list of changes. new_root_node:          The root of the NEW tree, used
//   to get current file content. dctx_binary_filepath:   Path to the NEW
//   .dircontxt binary for reading content.
//   data_section_start_offset_in_dctx_file: Byte offset where data begins.
//   old_version:            The previous version string (e.g., "V1.1").
//   new_version:            The new version string (e.g., "V1.2").
//
// Returns:
//   True if the diff file was generated successfully, false otherwise.
bool generate_diff_file(const char *diff_filepath, const DiffReport *report,
                        DirContextTreeNode *new_root_node,
                        const char *dctx_binary_filepath,
                        uint64_t data_section_start_offset_in_dctx_file,
                        const char *old_version, const char *new_version);

#endif // LLM_FORMATTER_H

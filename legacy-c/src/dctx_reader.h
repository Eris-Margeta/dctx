#ifndef DCTX_READER_H
#define DCTX_READER_H

#include "datatypes.h" // For DirContextTreeNode
#include <stdbool.h>
#include <stdio.h> // For FILE*

// --- Core .dircontxt Reading Functions ---

// Parses a .dircontxt binary file and reconstructs the directory tree in
// memory.
//
// Parameters:
//   dctx_filepath: Path to the .dircontxt binary file.
//   root_node_out: Pointer to a DirContextTreeNode pointer. On success, this
//   will
//                  be updated to point to the root of the reconstructed tree.
//                  The caller is responsible for freeing this tree using
//                  free_tree_recursive().
//   data_section_start_offset_out: (Optional) Pointer to a uint64_t to store
//   the byte offset
//                                  in the .dircontxt file where the actual file
//                                  data section begins.
//
// Returns:
//   True if parsing was successful, false otherwise (e.g., bad signature,
//   format error).
bool dctx_read_and_parse_header(const char *dctx_filepath,
                                DirContextTreeNode **root_node_out,
                                uint64_t *data_section_start_offset_out);

// Reads the raw content of a specific file from an opened .dircontxt file
// stream. Assumes the file stream `dctx_fp` is already opened and positioned
// correctly, and that `data_section_start_offset_in_file` is known.
//
// Parameters:
//   dctx_fp: An opened FILE stream for the .dircontxt file.
//   data_section_start_offset_in_file: The absolute byte offset where the data
//   section begins in dctx_fp. file_node_info: A DirContextTreeNode (for a
//   file) containing its content_offset_in_data_section
//                   and content_size.
//   buffer_out: A caller-allocated buffer to store the file content.
//   buffer_size: The size of buffer_out. Must be >=
//   file_node_info->content_size.
//
// Returns:
//   True if content was read successfully into buffer_out, false on error
//   (e.g., seek error, read error). The content in buffer_out is NOT
//   null-terminated by this function unless it was in the original file.
bool dctx_read_file_content(FILE *dctx_fp,
                            uint64_t data_section_start_offset_in_file,
                            const DirContextTreeNode *file_node_info,
                            char *buffer_out, size_t buffer_size);

// A convenience function to open, parse header, read file content, and close.
// The caller is responsible for freeing `content_buffer_out` if it's allocated
// by this function (or if this function requires the caller to pre-allocate it
// and pass its size). For now, let's assume caller pre-allocates and
// `content_buffer_out` is filled.
//
// Parameters:
//   dctx_filepath: Path to the .dircontxt binary file.
//   target_relative_path: The relative path of the file to extract from the
//   archive. content_buffer_out: Pointer to a char pointer. If successful, will
//   point to dynamically allocated buffer
//                       containing file content. Caller must free.
//   content_size_out: Pointer to store the size of the extracted content.
//
// Returns:
//   True if successful, false otherwise.
// bool dctx_extract_file_by_path(const char* dctx_filepath,
//                                const char* target_relative_path,
//                                char** content_buffer_out,
//                                uint64_t* content_size_out);
// NOTE: For the LLM formatter, we'll iterate the tree and use
// dctx_read_file_content directly.
//       The dctx_extract_file_by_path might be useful for other utilities
//       later.

#endif // DCTX_READER_H

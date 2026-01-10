#ifndef VERSION_H
#define VERSION_H

#include <stdbool.h>
#include <stddef.h> // For size_t

// --- Versioning Constants ---
#define VERSION_HEADER_PREFIX "[DIRCONTXT_LLM_SNAPSHOT_"
#define VERSION_HEADER_SUFFIX "]"

// --- Public Functions ---

// Reads the first line of a .llmcontext.txt file and extracts the version
// string. For example, from "[DIRCONTXT_LLM_SNAPSHOT_V1.2]", it extracts
// "V1.2".
//
// Parameters:
//   filepath:       The path to the .llmcontext.txt file to read.
//   version_out:    A buffer to store the extracted version string.
//   buffer_size:    The size of the version_out buffer.
//
// Returns:
//   True if the version was successfully found and parsed, false otherwise.
bool parse_version_from_file(const char *filepath, char *version_out,
                             size_t buffer_size);

// Calculates the next incremental version string based on the old one.
// - If old_version is "V1", the new version will be "V1.1".
// - If old_version is "V1.1", the new version will be "V1.2".
// - If the format is unrecognized, it defaults to "V1".
//
// Parameters:
//   old_version:      The version string to increment (e.g., "V1.1").
//   new_version_out:  A buffer to store the newly calculated version string.
//   buffer_size:      The size of the new_version_out buffer.
void calculate_next_version(const char *old_version, char *new_version_out,
                            size_t buffer_size);

#endif // VERSION_H

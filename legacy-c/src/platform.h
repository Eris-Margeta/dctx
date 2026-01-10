#ifndef PLATFORM_H
#define PLATFORM_H

#include <stdbool.h>  // For bool
#include <stdint.h>   // For uint64_t
#include <sys/stat.h> // For struct stat, S_ISDIR, S_ISREG
#include <time.h>     // For time_t

// We'll primarily be on POSIX-like systems (macOS, Linux) for now.
// Windows-specific code would be guarded by #ifdef _WIN32 etc.

// --- Filesystem Operations ---

// Get file status (last modified time, type, etc.)
// Returns 0 on success, -1 on error.
// On POSIX, this will wrap stat().
int platform_get_file_stat(const char *path, struct stat *stat_buf);

// Check if a path is a directory from a stat buffer
bool platform_is_dir(const struct stat *stat_buf);

// Check if a path is a regular file from a stat buffer
bool platform_is_reg_file(const struct stat *stat_buf);

// Get last modified time as a Unix timestamp (seconds since epoch)
uint64_t platform_get_mod_time(const struct stat *stat_buf);

// Resolve a path to its absolute form.
// `resolved_path` buffer should be at least MAX_PATH_LEN.
// Returns true on success, false on failure.
bool platform_resolve_path(const char *path, char *resolved_path_out,
                           size_t out_buffer_size);

// Get the base name of a path (e.g., "file.txt" from "/path/to/file.txt")
// The returned pointer is to a part of the input string or a static buffer;
// caller should copy if modification or long-term storage is needed.
// For simplicity, we'll just use a basic version.
const char *platform_get_basename(const char *path);

// Get the directory name of a path (e.g. "/path/to" from "/path/to/file.txt")
// The caller is responsible for freeing the returned string if it's dynamically
// allocated. For POSIX, this might wrap dirname(). Note: dirname can modify its
// input. For a safer approach, we'll implement a custom one or ensure proper
// usage.
char *platform_get_dirname(const char *path);

// --- Path Manipulation ---

// Join two path components with the correct separator.
// `base_path` + `separator` + `component` -> `result_path`
// `result_path_buffer` should be large enough.
// Returns true on success, false on failure (e.g., buffer too small).
bool platform_join_paths(const char *base_path, const char *component,
                         char *result_path_buffer, size_t buffer_size);

// --- NEW: Clipboard Functionality ---

// Copies the given text content to the system clipboard.
// This requires platform-specific command-line tools to be installed
// (e.g., 'pbcopy' on macOS, 'xclip' or 'wl-copy' on Linux).
// Returns true on success, false on failure.
bool platform_copy_to_clipboard(const char *text);

// Define the directory separator character for the current platform
#ifdef _WIN32
#define PLATFORM_DIR_SEPARATOR '\\'
#define PLATFORM_DIR_SEPARATOR_STR "\\"
#else
#define PLATFORM_DIR_SEPARATOR '/'
#define PLATFORM_DIR_SEPARATOR_STR "/"
#endif

#endif // PLATFORM_H

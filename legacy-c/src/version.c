#include "version.h"
#include "utils.h" // For read_line_from_file, safe_strncpy, log_error

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

// --- Public Function Implementations ---

bool parse_version_from_file(const char *filepath, char *version_out,
                             size_t buffer_size) {
  if (filepath == NULL || version_out == NULL || buffer_size == 0) {
    return false;
  }
  version_out[0] = '\0'; // Ensure buffer is empty on failure

  FILE *fp = fopen(filepath, "r");
  if (fp == NULL) {
    // This is not necessarily an error, the file just might not exist.
    return false;
  }

  char *first_line = read_line_from_file(fp);
  fclose(fp);

  if (first_line == NULL) {
    log_error("Could not read the first line from existing file: %s", filepath);
    return false;
  }

  bool success = false;
  const char *prefix = VERSION_HEADER_PREFIX;
  const char *suffix = VERSION_HEADER_SUFFIX;

  // Check if the line starts with our prefix and ends with our suffix
  if (strncmp(first_line, prefix, strlen(prefix)) == 0) {
    char *version_start = first_line + strlen(prefix);
    char *version_end = strstr(version_start, suffix);

    if (version_end != NULL) {
      // Found a valid version string
      *version_end = '\0'; // Terminate the string at the start of the suffix
      safe_strncpy(version_out, version_start, buffer_size);
      success = true;
    }
  }

  free(first_line);

  if (!success) {
    log_error("Failed to parse version header from file: %s", filepath);
  }

  return success;
}

void calculate_next_version(const char *old_version, char *new_version_out,
                            size_t buffer_size) {
  if (old_version == NULL || new_version_out == NULL || buffer_size == 0) {
    return;
  }

  int major = 0;
  int minor = 0;

  // Try to parse "Vx.y" format
  if (sscanf(old_version, "V%d.%d", &major, &minor) == 2) {
    // Successfully parsed Vmajor.minor, so we increment the minor part
    minor++;
    snprintf(new_version_out, buffer_size, "V%d.%d", major, minor);
  }
  // Try to parse "Vx" format
  else if (sscanf(old_version, "V%d", &major) == 1) {
    // Successfully parsed Vmajor, so this is the first increment. Start at .1
    minor = 1;
    snprintf(new_version_out, buffer_size, "V%d.%d", major, minor);
  }
  // If parsing fails, default to "V1"
  else {
    log_error("Unrecognized version format '%s'. Defaulting to V1.",
              old_version);
    safe_strncpy(new_version_out, "V1", buffer_size);
  }
}

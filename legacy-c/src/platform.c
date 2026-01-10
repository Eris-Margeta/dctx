#include "platform.h"
#include "datatypes.h" // For MAX_PATH_LEN
#include "utils.h"     // For safe_strncpy and logging functions

#include <errno.h>
#include <libgen.h> // For basename
#include <stdio.h>
#include <stdlib.h> // For realpath, malloc, free, getenv
#include <string.h> // For strrchr, strlen, strcpy

// --- Filesystem Operations ---

int platform_get_file_stat(const char *path, struct stat *stat_buf) {
  if (stat(path, stat_buf) != 0) {
    return -1;
  }
  return 0;
}

bool platform_is_dir(const struct stat *stat_buf) {
  return S_ISDIR(stat_buf->st_mode);
}

bool platform_is_reg_file(const struct stat *stat_buf) {
  return S_ISREG(stat_buf->st_mode);
}

uint64_t platform_get_mod_time(const struct stat *stat_buf) {
  return (uint64_t)stat_buf->st_mtime;
}

bool platform_resolve_path(const char *path, char *resolved_path_out,
                           size_t out_buffer_size) {
  char *real_path_result = realpath(path, NULL);
  if (real_path_result == NULL) {
    return false;
  }

  if (strlen(real_path_result) + 1 > out_buffer_size) {
    fprintf(stderr, "Error: Resolved path exceeds buffer size.\n");
    free(real_path_result);
    return false;
  }

  safe_strncpy(resolved_path_out, real_path_result, out_buffer_size);
  free(real_path_result);
  return true;
}

const char *platform_get_basename(const char *path) {
  if (path == NULL || path[0] == '\0')
    return ".";

  const char *last_slash = strrchr(path, PLATFORM_DIR_SEPARATOR);
  if (last_slash) {
    // If the slash is the last character, we need to find the segment before
    // it.
    if (*(last_slash + 1) == '\0') {
      // This requires a mutable copy to strip trailing slashes, which is
      // complex. The get_directory_basename wrapper in utils.c handles this
      // safely. For this low-level function, we assume a clean path.
    }
    return last_slash + 1;
  }
  return path;
}

// MODIFIED: Replaced the standard library dirname() with a robust custom
// implementation.
char *platform_get_dirname(const char *path) {
  if (path == NULL || path[0] == '\0') {
    return strdup(".");
  }

  char *path_copy = strdup(path);
  if (!path_copy) {
    perror("platform_get_dirname - strdup");
    return NULL;
  }

  // Remove trailing slashes
  size_t len = strlen(path_copy);
  while (len > 1 && path_copy[len - 1] == PLATFORM_DIR_SEPARATOR) {
    path_copy[--len] = '\0';
  }

  // Find the last directory separator
  char *last_slash = strrchr(path_copy, PLATFORM_DIR_SEPARATOR);

  if (last_slash == NULL) {
    // No separator found, the parent is the current directory "."
    free(path_copy);
    return strdup(".");
  }

  if (last_slash == path_copy) {
    // The only separator is at the beginning, so parent is "/"
    *(last_slash + 1) = '\0';
  } else {
    // Terminate the string at the slash to get the parent path
    *last_slash = '\0';
  }

  // We can return path_copy directly since we're giving up ownership of it.
  return path_copy;
}

// --- Path Manipulation ---

bool platform_join_paths(const char *base_path, const char *component,
                         char *result_path_buffer, size_t buffer_size) {
  if (!base_path || !component || !result_path_buffer) {
    return false;
  }

  size_t base_len = strlen(base_path);
  size_t component_len = strlen(component);

  if (base_len + component_len + 2 > buffer_size) {
    fprintf(stderr, "Error: Joined path exceeds buffer size.\n");
    return false;
  }

  strcpy(result_path_buffer, base_path);

  if (base_len > 0 &&
      result_path_buffer[base_len - 1] != PLATFORM_DIR_SEPARATOR &&
      component[0] != PLATFORM_DIR_SEPARATOR) {
    result_path_buffer[base_len] = PLATFORM_DIR_SEPARATOR;
    result_path_buffer[base_len + 1] = '\0';
  }

  strcat(result_path_buffer, component);
  return true;
}

// --- NEW: Clipboard Implementation ---
bool platform_copy_to_clipboard(const char *text) {
  const char *command = NULL;
#if defined(__APPLE__)
  command = "pbcopy";
#elif defined(__linux__) || defined(__unix__)
  if (getenv("WAYLAND_DISPLAY")) {
    command = "wl-copy";
  } else if (getenv("DISPLAY")) {
    command = "xclip -selection clipboard";
  } else {
    log_error("Clipboard error: Cannot detect WAYLAND or X11 display server.");
    return false;
  }
#else
  log_error("Clipboard functionality is not supported on this platform.");
  return false;
#endif

  FILE *pipe = popen(command, "w");
  if (pipe == NULL) {
    log_error("Clipboard error: Failed to open pipe to '%s'. Is the required "
              "utility (pbcopy, wl-copy, xclip) installed?",
              command);
    return false;
  }

  if (fputs(text, pipe) == EOF) {
    log_error("Clipboard error: Failed to write data to the clipboard pipe.");
    pclose(pipe);
    return false;
  }

  if (pclose(pipe) != 0) {
    log_error("Clipboard error: The clipboard command returned a non-zero exit "
              "status.");
    return false;
  }

  log_info("Content copied to clipboard.");
  return true;
}

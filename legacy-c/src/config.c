#include "config.h"
#include "datatypes.h" // For MAX_PATH_LEN
#include "platform.h"  // For path joining
#include "utils.h"     // For logging and string utils

#include <ctype.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

// --- Static Helper Function Declarations ---

// Sets the hardcoded default values for the AppConfig struct.
static void set_default_config(AppConfig *config);

// Parses a single "KEY=VALUE" line from the config file.
static void parse_config_line(const char *line, AppConfig *config);

// --- Public Function Implementation ---

void load_app_config(AppConfig *config_out) {
  // 1. Start with safe, hardcoded defaults.
  set_default_config(config_out);

  // 2. Determine the path to the global config file.
  const char *home_dir = getenv("HOME");
  if (!home_dir) {
    log_info("Could not find HOME environment variable. Using default "
             "configuration.");
    return;
  }

  char config_path[MAX_PATH_LEN];
  snprintf(config_path, MAX_PATH_LEN, "%s/.config/dircontxt/config", home_dir);

  // 3. Attempt to open and read the file.
  FILE *fp = fopen(config_path, "r");
  if (fp == NULL) {
    // This is not an error; it just means the user hasn't created a global
    // config.
    log_debug("No global config file found at %s. Using default configuration.",
              config_path);
    return;
  }

  log_info("Loading configuration from: %s", config_path);
  char *line;
  while ((line = read_line_from_file(fp)) != NULL) {
    parse_config_line(line, config_out);
    free(line);
  }

  fclose(fp);
}

// --- Static Helper Function Implementations ---

static void set_default_config(AppConfig *config) {
  if (config == NULL)
    return;
  // The default behavior is to create both files.
  config->output_mode = OUTPUT_MODE_BOTH;
}

static void parse_config_line(const char *orig_line, AppConfig *config) {
  if (orig_line == NULL || config == NULL)
    return;

  char line_buffer[MAX_PATH_LEN];
  safe_strncpy(line_buffer, orig_line, MAX_PATH_LEN);
  trim_trailing_newline(line_buffer);

  const char *line = line_buffer;
  // Trim leading whitespace
  while (isspace((unsigned char)*line))
    line++;

  // Skip comments and empty lines
  if (line[0] == '\0' || line[0] == '#') {
    return;
  }

  char *separator = strchr(line, '=');
  if (separator == NULL) {
    log_error("Warning: Invalid line in config file (missing '='): %s", line);
    return;
  }

  // Split into key and value
  *separator = '\0'; // Null-terminate the key
  char *key = (char *)line;
  char *value = separator + 1;

  // Trim whitespace from key
  char *end = key + strlen(key) - 1;
  while (end > key && isspace((unsigned char)*end)) {
    *end-- = '\0';
  }

  // Trim whitespace from value
  while (isspace((unsigned char)*value))
    value++;
  end = value + strlen(value) - 1;
  while (end > value && isspace((unsigned char)*end)) {
    *end-- = '\0';
  }

  // --- Process the Key-Value Pair ---
  if (strcmp(key, "OUTPUT_MODE") == 0) {
    if (strcmp(value, "text") == 0) {
      config->output_mode = OUTPUT_MODE_TEXT_ONLY;
      log_debug("Config: Output mode set to TEXT_ONLY.");
    } else if (strcmp(value, "binary") == 0) {
      config->output_mode = OUTPUT_MODE_BINARY_ONLY;
      log_debug("Config: Output mode set to BINARY_ONLY.");
    } else if (strcmp(value, "both") == 0) {
      config->output_mode = OUTPUT_MODE_BOTH;
      log_debug("Config: Output mode set to BOTH.");
    } else {
      log_error("Warning: Unknown value for OUTPUT_MODE in config: '%s'. Using "
                "default.",
                value);
    }
  } else {
    log_error("Warning: Unknown key in config file: '%s'", key);
  }
}

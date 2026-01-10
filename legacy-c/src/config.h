#ifndef CONFIG_H
#define CONFIG_H

#include <stdbool.h>

// --- Application Configuration ---

// Enum to define the file output behavior
typedef enum {
  OUTPUT_MODE_BOTH,
  OUTPUT_MODE_TEXT_ONLY,
  OUTPUT_MODE_BINARY_ONLY
} OutputMode;

// Structure to hold all application settings loaded from the config file
typedef struct {
  OutputMode output_mode;
  // Future settings can be added here, e.g.:
  // bool follow_symlinks;
} AppConfig;

// --- Public Functions ---

// Loads the application configuration.
// It first sets safe default values and then attempts to read and parse
// the global config file (~/.config/dircontxt/config) to override them.
//
// Parameters:
//   config_out: A pointer to an AppConfig struct that will be populated.
void load_app_config(AppConfig *config_out);

#endif // CONFIG_H

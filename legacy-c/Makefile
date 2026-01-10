# Compiler and C_STANDARD
CC = clang
C_STANDARD = -std=c11

# Tools
RM = rm -rf

# Directories
SRC_DIR = src
BUILD_DIR = build
OBJ_DIR = $(BUILD_DIR)/obj
TARGET_DIR = $(BUILD_DIR)/bin

# Target executable name
TARGET = $(TARGET_DIR)/dircontxt

# Source files (find all .c files in SRC_DIR)
SRCS = $(wildcard $(SRC_DIR)/*.c)

# Object files (replace .c with .o and put them in OBJ_DIR)
OBJS = $(patsubst $(SRC_DIR)/%.c, $(OBJ_DIR)/%.o, $(SRCS))

# Compilation flags
# -I$(SRC_DIR): Add src directory to include path for local headers
# -g: Add debug information
# -Wall, -Wextra, -pedantic: Enable comprehensive warnings for robust code
CFLAGS_DEBUG = $(C_STANDARD) -g -Wall -Wextra -pedantic -I$(SRC_DIR)
CFLAGS_RELEASE = $(C_STANDARD) -O2 -Wall -I$(SRC_DIR) -DNDEBUG

# Default to debug flags
CFLAGS = $(CFLAGS_DEBUG)

# Linker flags
LDFLAGS =

# Phony targets (targets that don't represent actual files)
.PHONY: all clean test run debug_run help release

# Default target (called when you just run `make`)
all: $(TARGET)

# Rule to link the target executable
$(TARGET): $(OBJS)
	@mkdir -p $(TARGET_DIR)
	@echo "LD $@"
	$(CC) $(OBJS) -o $@ $(LDFLAGS)

# Rule to compile .c files into .o files
$(OBJ_DIR)/%.o: $(SRC_DIR)/%.c | $(OBJ_DIR) # The "| $(OBJ_DIR)" is an order-only prerequisite
	@echo "CC $<"
	$(CC) $(CFLAGS) -c $< -o $@

# Rule to create the object directory
$(OBJ_DIR):
	@mkdir -p $(OBJ_DIR)

# Clean up all build artifacts and test outputs
clean:
	@echo "Cleaning build artifacts and test files..."
	$(RM) $(BUILD_DIR)
	$(RM) test_dir
	$(RM) test_dir.dircontxt
	$(RM) test_dir.llmcontext.txt

# Comprehensive test run to validate advanced ignore logic
test: $(TARGET)
	@echo "--- Setting up comprehensive test environment ---"
	# Clean up previous runs
	$(RM) test_dir test_dir.dircontxt test_dir.llmcontext.txt

	# Create directories to test default, wildcard, and specific ignores
	mkdir -p test_dir/src
	mkdir -p test_dir/.git # Should be ignored by default
	mkdir -p test_dir/node_modules/some-lib # Should be ignored by default
	mkdir -p test_dir/build/logs # Should be ignored by a directory rule

	# Create test files
	echo "Main source file" > test_dir/src/main.c
	echo "README" > test_dir/README.md
	echo "Git config data" > test_dir/.git/config
	echo "A library file" > test_dir/node_modules/some-lib/index.js
	echo "A generic log" > test_dir/app.log # Should be ignored by wildcard *.log
	echo "A build log" > test_dir/build/logs/output.log # Should be ignored by build/ rule
	echo "An important log" > test_dir/build/important.log # Should be re-included by negation

	# Create the project-specific .dircontxtignore file
	@echo "--- Creating .dircontxtignore with advanced rules ---"
	echo "# Ignore build artifacts and all log files" > test_dir/.dircontxtignore
	echo "build/" >> test_dir/.dircontxtignore
	echo "*.log" >> test_dir/.dircontxtignore
	echo "" >> test_dir/.dircontxtignore
	echo "# Negation Rule: Re-include the important log file" >> test_dir/.dircontxtignore
	echo "!build/important.log" >> test_dir/.dircontxtignore

	@echo "--- Running $(TARGET) on ./test_dir ---"
	$(TARGET) test_dir
	@echo "--- Test run complete ---"
	@echo
	@echo "=> VERIFICATION:"
	@echo "   Check 'test_dir.llmcontext.txt' to confirm the following:"
	@echo "   - INCLUDED: src/main.c, README.md, build/important.log"
	@echo "   - IGNORED: .git/, node_modules/, build/logs/, app.log"
	@echo

# 'run' is now a convenient alias for 'test'
run: test

# Run with debug logging enabled (ensures CFLAGS are set for debug)
debug_run:
	@echo "Building and running in debug mode..."
	$(MAKE) CFLAGS="$(CFLAGS_DEBUG)" run

# Build for release (optimized, no debug symbols, NDEBUG defined)
release: clean
	@echo "Building for release..."
	$(MAKE) CFLAGS="$(CFLAGS_RELEASE)" all
	@echo "Stripping debug symbols from the executable..."
	strip $(TARGET)
	@echo "Release build complete. Executable at $(TARGET)"

# Help target to display available commands
help:
	@echo "Available targets:"
	@echo "  all         : Build the $(TARGET) executable (default, debug build)."
	@echo "  clean       : Remove all build artifacts and test outputs."
	@echo "  test        : Set up a comprehensive test case and run the program."
	@echo "  run         : Alias for 'test'."
	@echo "  debug_run   : Force a debug build and run the test case."
	@echo "  release     : Build an optimized release executable with debug symbols stripped."
	@echo "  help        : Show this help message."

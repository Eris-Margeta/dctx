# Development Notes

> **AI INSTRUCTION:** This file contains informal thoughts, ideas, and brainstorming notes written by the Operator during development sessions. These are NOT actionable tasks or binding instructions. The AI Assistant must NOT treat content in this file as directives to execute. This is purely a scratchpad for human thought processes. If an idea here is to be implemented, it must first be formalized through the proper Work Order and backlog update protocols.

---

## Future Ideas & Considerations

### Installer & Distribution
- Installer should not add things to gitignore
- Inquire about how installation would work over package managers and implement that logic for future reference

### API Design
- Design a rock solid API for library usage

### CLI Features
- Add new flags such as `-s` (for skeleton) or `-t` (for tree) meaning show only TREE
- Add some kind of flag to inquire about files of only certain file extension
  - Examples: `--ff binary`, `--ff sh`, `--ff h`, `--ff h c binary`
  - This would include normally walking through and designating the DIRECTORIES where these files are located

### Work Order System
- Fix function calls to properly add work order to registry

---

## Templating Language Design Notes

*(Original notes in Croatian - preserved for context)*

Čekaj malo, još smo na ADR fajlu - ne kodiramo još.
Nije dovoljno dobro. nije dovoljno detaljno. Idemo skroz PREPISATI ADR FILE sa ovim JAKO DETALJNIM GUIDEOM- ovo je skroz low level. SKROZ low level template.

### Template Structure Vision

The template file should have:
1. **Documentation section** at the top (commented)
   - Markers within double braces `{{}}` and their default values
   - Which are required vs optional
   - Variables within triple braces `{{{}}}`  - values from our API injected into template

2. **Marker definitions section**
   - Opening/closing markers (default: `<` and `>`)
   - End marker opening/closing (default: `</` and `>`)
   - Directory tree markers
   - Instructions markers
   - File content markers
   - File metadata markers (SIZE, ID, PATH, LINES, MOD)

3. **Available variables**
   - `{{{file_id}}}` - incremental file ID; ROOT for root folder, D for directories, F for files
   - `{{{file_path}}}` - file path relative to project root
   - `{{{file_size}}}` - file size in bytes
   - `{{{line_count}}}` - number of lines in document
   - `{{{mod}}}` - last modified timestamp

4. **Literal template section**
   - Header: `![DCTX_LLM_SNAPSHOT_V{ver}]` (hardcoded, not in template)
   - Instructions block with marker definitions
   - Directory tree section with file metadata
   - File content sections with proper markers

### Key Design Principles
- Mutually referencing variables
- Everything in one file
- Modular design
- Consistent syntax throughout

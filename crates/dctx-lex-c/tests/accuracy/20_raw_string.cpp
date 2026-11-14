const char *schema = R"x(
struct ghost_raw { int fake; };
class ghost_class {};
int ghost_function();
)x";
int load_schema();

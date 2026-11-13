#if 0
struct Disabled { int hidden; };
#define DISABLED_LIMIT 9
int disabled(void);
#else
int enabled(void);
#endif
int unconditional(void);

#define IDENTITY(value) (value)
struct Boxed { int value; };
static int helper(int value) { return IDENTITY(value); }
int café(void) { return helper(1); }

/// File demo is a sample.

#include <stdio.h>
#include "box.h"
#define LIMIT 3
#define SHOW(x) ((x) + 1)
#ifdef EXTRA
#include "extra.h"
#endif
#if 0
int configured(void);
#endif
/* func hidden_in_comment(void) {} */
/// Add adds two ints.
int add(int a, int b) { return a + b; }
typedef struct Box { int width; int height; } Box;
enum Color { Red = 1, Blue };
union Word { int as_int; char bytes[4]; };
static int hidden_fn(void) { return LIMIT; }
int spl\
ice(int v) { return v; }
int main(void) {
  char *s = "int decoy(void) { return 1; }";
  struct Box box;
  struct Box *node;
  box.width = 1;
  node->width = add(1, SHOW(2));
  printf("%s\n", s);
  return box.width;
}

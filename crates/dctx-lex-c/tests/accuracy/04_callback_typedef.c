typedef int (*Comparator)(const void *left, const void *right);
typedef void (*Cleanup)(void *resource);
Comparator compare_items;

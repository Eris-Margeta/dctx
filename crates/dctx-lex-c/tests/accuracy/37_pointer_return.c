int (*select_handler(int mode))(int code);
int invoke(int (*handler)(int), int value) { return handler(value); }

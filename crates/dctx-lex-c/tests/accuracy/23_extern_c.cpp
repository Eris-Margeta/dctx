extern "C" {
int open_device(const char *name);
struct Device { int descriptor; };
}
extern "C" int close_device(int descriptor);

#if 0
#if FEATURE
int hidden_nested(void);
#endif
#elif 0
int hidden_alternate(void);
#else
int selected(void);
#endif
#if FEATURE
int feature_on(void);
#else
int feature_off(void);
#endif

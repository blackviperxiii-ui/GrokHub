#pragma once
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

char *grokhub_hub_kind(void);
uint16_t grokhub_default_port(void);
char *grokhub_make_pair_code(void);
char *grokhub_normalize_code(const char *code);
char *grokhub_imagine_model(const char *user);
char *grokhub_voice_model(const char *user);
int grokhub_forbidden(const char *cmd);
char *grokhub_slash_kind(const char *line);
void grokhub_string_free(char *s);

#ifdef __cplusplus
}
#endif

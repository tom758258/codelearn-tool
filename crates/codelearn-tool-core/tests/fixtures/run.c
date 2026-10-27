/* Synthetic, bounded fixture for the Run API. No user program or private data. */
#define _POSIX_C_SOURCE 200809L
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#ifdef _WIN32
#include <windows.h>
#include <wchar.h>
#include <fcntl.h>
#include <io.h>
static void pause_ms(void) { Sleep(10); }
#else
#include <unistd.h>
#include <time.h>
static void pause_ms(void) {
    struct timespec delay = {0, 10000000};
    nanosleep(&delay, NULL);
}
#endif

static void heartbeat(void) {
    /* Always self-terminate even if the supervisor under test fails. */
    for (int i = 0; i < 1500; ++i) {
        FILE *file = fopen("heartbeat", "wb");
        if (!file) exit(90);
        fprintf(file, "%d", i);
        fclose(file);
        pause_ms();
    }
    exit(0);
}

static void descendant(int escape) {
#ifdef _WIN32
    (void)escape;
    wchar_t executable[32768];
    wchar_t command[32780];
    if (!GetModuleFileNameW(NULL, executable, 32768)) exit(91);
    swprintf(command, 32780, L"\"%ls\" --child", executable);
    STARTUPINFOW startup = {0};
    PROCESS_INFORMATION process = {0};
    startup.cb = sizeof(startup);
    startup.dwFlags = STARTF_USESTDHANDLES;
    startup.hStdInput = GetStdHandle(STD_INPUT_HANDLE);
    startup.hStdOutput = GetStdHandle(STD_OUTPUT_HANDLE);
    startup.hStdError = GetStdHandle(STD_ERROR_HANDLE);
    if (!CreateProcessW(executable, command, NULL, NULL, TRUE, CREATE_NO_WINDOW,
                        NULL, NULL, &startup, &process)) exit(92);
    CloseHandle(process.hThread);
    CloseHandle(process.hProcess);
#else
    pid_t child = fork();
    if (child < 0) exit(91);
    if (child == 0) {
        if (escape && setsid() == -1) exit(92);
        heartbeat();
    }
    FILE *pid_file = fopen("child-pid", "wb");
    if (!pid_file) exit(93);
    fprintf(pid_file, "%ld", (long)child);
    fclose(pid_file);
#endif
    for (int i = 0; i < 500; ++i) {
        FILE *file = fopen("heartbeat", "rb");
        if (file) { fclose(file); return; }
        pause_ms();
    }
    exit(94);
}

int main(int argc, char **argv) {
#ifdef _WIN32
    _setmode(_fileno(stdin), _O_BINARY);
    _setmode(_fileno(stdout), _O_BINARY);
    _setmode(_fileno(stderr), _O_BINARY);
#endif
    if (argc == 2 && strcmp(argv[1], "--child") == 0) heartbeat();
    char mode[32] = {0};
    FILE *config = fopen("mode", "rb");
    if (!config) return 80;
    if (!fgets(mode, sizeof(mode), config)) return 81;
    fclose(config);
    unsigned char block[4096];
    memset(block, 0xff, sizeof(block));
    if (!strcmp(mode, "exact") || !strcmp(mode, "overflow")) {
        descendant(0);
        for (int i = 0; i < 13 * 256; ++i) fwrite(block, 1, sizeof(block), stdout);
        for (int i = 0; i < 12 * 256; ++i) fwrite(block, 1, sizeof(block), stderr);
        if (!strcmp(mode, "overflow")) fputc(0, stderr);
        fflush(stdout);
        fflush(stderr);
        if (!strcmp(mode, "overflow")) {
            for (int i = 0; i < 1500; ++i) pause_ms();
        }
        return 0;
    }
    if (!strcmp(mode, "wait") || !strcmp(mode, "descendant") || !strcmp(mode, "escape")) {
        descendant(!strcmp(mode, "escape"));
        FILE *ready = fopen("ready", "wb");
        if (!ready) return 82;
        fclose(ready);
        if (!strcmp(mode, "wait")) {
            for (int i = 0; i < 1500; ++i) pause_ms();
        }
        return 0;
    }
    if (!strcmp(mode, "close")) {
        fclose(stdin);
        return 7;
    }
    /* Fill both output pipes before consuming a larger-than-pipe stdin. */
    for (int i = 0; i < 32; ++i) {
        fwrite(block, 1, sizeof(block), stdout);
        fwrite(block, 1, sizeof(block), stderr);
    }
    size_t count;
    while ((count = fread(block, 1, sizeof(block), stdin)) > 0) {
        fwrite(block, 1, count, stdout);
    }
    FILE *input = fopen("input/data.bin", "rb");
    if (!input) return 83;
    while ((count = fread(block, 1, sizeof(block), input)) > 0) {
        fwrite(block, 1, count, stdout);
    }
    fclose(input);
    input = fopen("input/data.bin", "wb");
    if (!input) return 84;
    fputs("modified copy", input);
    fclose(input);
    input = fopen("unselected", "rb");
    if (input) { fclose(input); return 85; }
    return !strcmp(mode, "exit7") ? 7 : 0;
}

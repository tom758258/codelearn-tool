#define _POSIX_C_SOURCE 200809L
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <wchar.h>
#ifdef _WIN32
#include <windows.h>
#include <tlhelp32.h>
#include <io.h>
#include <fcntl.h>
static void tick(void) { Sleep(10); }
static unsigned long process_id(void) { return GetCurrentProcessId(); }
static unsigned long parent_id(void) {
    PROCESSENTRY32 entry; memset(&entry, 0, sizeof entry); entry.dwSize = sizeof entry;
    HANDLE snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
    unsigned long parent = 0;
    if (snapshot != INVALID_HANDLE_VALUE) {
        if (Process32First(snapshot, &entry)) do {
            if (entry.th32ProcessID == GetCurrentProcessId()) { parent = entry.th32ParentProcessID; break; }
        } while (Process32Next(snapshot, &entry));
        CloseHandle(snapshot);
    }
    return parent;
}
#else
#include <unistd.h>
#include <time.h>
static void tick(void) { struct timespec t = {0, 10000000}; nanosleep(&t, NULL); }
static unsigned long process_id(void) { return (unsigned long)getpid(); }
static unsigned long parent_id(void) { return (unsigned long)getppid(); }
#endif
static void number(const char *path, unsigned long value) {
    FILE *f = fopen(path, "w"); if (f) { fprintf(f, "%lu", value); fclose(f); }
}
static int exists(const char *path) { FILE *f = fopen(path, "r"); if (!f) return 0; fclose(f); return 1; }
static void mode(char *value) {
    FILE *f = fopen("mode", "r"); value[0] = 0;
    if (f) { if (!fgets(value, 32, f)) value[0] = 0; fclose(f); }
}
__attribute__((constructor)) static void before_main(void) {
    char value[32]; mode(value);
    FILE *pidfile = fopen("debugger.pid", "wx");
    if (pidfile) { fprintf(pidfile, "%lu", parent_id()); fclose(pidfile); }
    if (!strcmp(value, "premain")) exit(19);
    if (!strcmp(value, "startup")) {
        number("startup-wait", 1);
        for (int i = 0; i < 500 && !exists("allow-main"); i++) tick();
        number("passed-startup", 1);
        if (!exists("allow-main")) exit(97);
    }
}
static int inner(int value) {
    int adjusted = value + 3; /* inner-entry */
    return adjusted * 2; /* inner-return */
}
static int outer(int seed) {
    int result = inner(seed);
    return result + 1;
}
static void heartbeat(void) {
    number("child.pid", process_id());
    for (unsigned long i = 0; i < 3000; i++) { number("heartbeat", i); tick(); }
}
static void descendant(void) {
#ifdef _WIN32
    wchar_t path[32768], command[32780];
    GetModuleFileNameW(NULL, path, 32768);
    swprintf(command, 32780, L"\"%ls\" --child", path);
    STARTUPINFOW start; PROCESS_INFORMATION info;
    memset(&start, 0, sizeof start); memset(&info, 0, sizeof info); start.cb = sizeof start;
    if (!CreateProcessW(path, command, NULL, NULL, FALSE, 0, NULL, NULL, &start, &info)) exit(80);
    CloseHandle(info.hThread); CloseHandle(info.hProcess);
#else
    pid_t pid = fork(); if (pid < 0) exit(80);
    if (!pid) { heartbeat(); _exit(0); }
#endif
}
int main(int argc, char **argv) {
#ifdef _WIN32
    _setmode(_fileno(stdin), _O_BINARY);
    _setmode(_fileno(stdout), _O_BINARY);
    _setmode(_fileno(stderr), _O_BINARY);
#endif
    if (argc == 2 && !strcmp(argv[1], "--child")) { heartbeat(); return 0; }
    char value[32]; mode(value); /* main-entry */
    number("inferior.pid", process_id());
    if (!strcmp(value, "flow")) {
        volatile int answer = outer(7); /* flow-call */
        (void)answer;
    } else if (!strcmp(value, "io")) {
        unsigned char data[1024]; size_t n;
        while ((n = fread(data, 1, sizeof data, stdin))) fwrite(data, 1, n, stdout);
        FILE *f = fopen("input/data.bin", "rb"); if (!f) return 81;
        while ((n = fread(data, 1, sizeof data, f))) fwrite(data, 1, n, stdout);
        fclose(f); fflush(stdout);
        const unsigned char error[] = {255, 0, 128, 42}; fwrite(error, 1, sizeof error, stderr); fflush(stderr);
        number("flushed", 1);
        while (!exists("finish")) tick();
        return 7;
    } else if (!strcmp(value, "overflow")) {
        unsigned char block[8192]; memset(block, 255, sizeof block);
        for (;;) { fwrite(block, 1, sizeof block, stdout); fwrite(block, 1, sizeof block, stderr); fflush(stdout); fflush(stderr); }
    } else if (!strcmp(value, "descendant")) descendant();
    for (unsigned long i = 0; i < 3000; i++) { number("running", i); tick(); }
    return 0;
}

/*
 * iSH (iOS) SIGSYS shim, preloaded into every program tina4 starts on iSH.
 *
 * The App Store build of iSH raises SIGSYS for a system call it does not
 * implement, where Linux returns ENOSYS, and leaves the call's result
 * register holding the syscall number. Default SIGSYS kills the program;
 * an ignored SIGSYS makes the call look like it succeeded with that number
 * (musl 1.2.6's pwrite tries pwritev2 first, so every write "wrote" 379
 * bytes and SQLite lost its data). This handler sets the result to -ENOSYS,
 * which is what Linux returns, so libc and the runtimes take their own
 * fallbacks. A handler does not survive exec, hence LD_PRELOAD.
 *
 * Built by build.rs for i686-unknown-linux-musl only; see ish_sigsys in
 * src/main.rs for how it is installed.
 */
#define _GNU_SOURCE
#include <errno.h>
#include <signal.h>
#include <string.h>
#include <ucontext.h>

static void on_sigsys(int sig, siginfo_t *info, void *context) {
    (void)sig;
    (void)info;
    ((ucontext_t *)context)->uc_mcontext.gregs[REG_EAX] = -ENOSYS;
}

__attribute__((constructor)) static void install_sigsys_handler(void) {
    struct sigaction action;
    memset(&action, 0, sizeof action);
    action.sa_sigaction = on_sigsys;
    action.sa_flags = SA_SIGINFO | SA_RESTART;
    sigaction(SIGSYS, &action, 0);
}

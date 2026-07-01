#include <errno.h>
#include <inttypes.h>
#include <linux/input-event-codes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <wayland-client.h>

#include "weston-test-client-protocol.h"

struct state {
    struct weston_test *test;
    uint32_t pointer_events;
    int32_t pointer_x;
    int32_t pointer_y;
};

static void pointer_position(void *data, struct weston_test *test, wl_fixed_t x, wl_fixed_t y)
{
    (void)test;
    struct state *state = data;
    state->pointer_events++;
    state->pointer_x = wl_fixed_to_int(x);
    state->pointer_y = wl_fixed_to_int(y);
}

static const struct weston_test_listener test_listener = {
    pointer_position,
};

static void global(
    void *data,
    struct wl_registry *registry,
    uint32_t name,
    const char *interface,
    uint32_t version)
{
    struct state *state = data;

    if (strcmp(interface, "weston_test") == 0) {
        state->test = wl_registry_bind(
            registry,
            name,
            &weston_test_interface,
            version < 1 ? version : 1);
        weston_test_add_listener(state->test, &test_listener, state);
    }
}

static void global_remove(void *data, struct wl_registry *registry, uint32_t name)
{
    (void)data;
    (void)registry;
    (void)name;
}

static const struct wl_registry_listener registry_listener = {
    global,
    global_remove,
};

static void stamp(uint32_t *hi, uint32_t *lo, uint32_t *nsec)
{
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    uint64_t sec = (uint64_t)ts.tv_sec;
    *hi = (uint32_t)(sec >> 32);
    *lo = (uint32_t)sec;
    *nsec = (uint32_t)ts.tv_nsec;
}

static uint64_t monotonic_nsec(void)
{
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return ((uint64_t)ts.tv_sec * 1000000000ULL) + (uint64_t)ts.tv_nsec;
}

static void sleep_millis(int millis)
{
    if (millis <= 0)
        return;
    struct timespec ts;
    ts.tv_sec = millis / 1000;
    ts.tv_nsec = (long)(millis % 1000) * 1000000L;
    while (nanosleep(&ts, &ts) == -1 && errno == EINTR) {
    }
}

static int key_code_for_char(char ch)
{
    switch (ch) {
    case 'a': case 'A': return KEY_A;
    case 'b': case 'B': return KEY_B;
    case 'c': case 'C': return KEY_C;
    case 'd': case 'D': return KEY_D;
    case 'e': case 'E': return KEY_E;
    case 'f': case 'F': return KEY_F;
    case 'g': case 'G': return KEY_G;
    case 'h': case 'H': return KEY_H;
    case 'i': case 'I': return KEY_I;
    case 'j': case 'J': return KEY_J;
    case 'k': case 'K': return KEY_K;
    case 'l': case 'L': return KEY_L;
    case 'm': case 'M': return KEY_M;
    case 'n': case 'N': return KEY_N;
    case 'o': case 'O': return KEY_O;
    case 'p': case 'P': return KEY_P;
    case 'q': case 'Q': return KEY_Q;
    case 'r': case 'R': return KEY_R;
    case 's': case 'S': return KEY_S;
    case 't': case 'T': return KEY_T;
    case 'u': case 'U': return KEY_U;
    case 'v': case 'V': return KEY_V;
    case 'w': case 'W': return KEY_W;
    case 'x': case 'X': return KEY_X;
    case 'y': case 'Y': return KEY_Y;
    case 'z': case 'Z': return KEY_Z;
    case ' ': return KEY_SPACE;
    case '0': return KEY_0;
    case '1': return KEY_1;
    case '2': return KEY_2;
    case '3': return KEY_3;
    case '4': return KEY_4;
    case '5': return KEY_5;
    case '6': return KEY_6;
    case '7': return KEY_7;
    case '8': return KEY_8;
    case '9': return KEY_9;
    default: return 0;
    }
}

static void send_key_press(struct weston_test *test, int key)
{
    uint32_t hi, lo, ns;
    stamp(&hi, &lo, &ns);
    weston_test_send_key(test, hi, lo, ns, key, WL_KEYBOARD_KEY_STATE_PRESSED);
    stamp(&hi, &lo, &ns);
    weston_test_send_key(test, hi, lo, ns, key, WL_KEYBOARD_KEY_STATE_RELEASED);
}

int main(int argc, char **argv)
{
    uint64_t process_start_monotonic_ns = monotonic_nsec();
    int x = argc > 1 ? atoi(argv[1]) : 240;
    int y = argc > 2 ? atoi(argv[2]) : 220;
    const char *text = argc > 3 ? argv[3] : NULL;
    const char *mode = argc > 4 ? argv[4] : "";
    int repeat_count = argc > 5 ? atoi(argv[5]) : 1;
    int repeat_delay_ms = argc > 6 ? atoi(argv[6]) : 0;
    if (repeat_count < 1)
        repeat_count = 1;
    if (repeat_count > 64)
        repeat_count = 64;
    if (repeat_delay_ms < 0)
        repeat_delay_ms = 0;
    if (repeat_delay_ms > 1000)
        repeat_delay_ms = 1000;
    int send_enter = strcmp(mode, "enter") == 0;
    int scroll_only = strcmp(mode, "scroll-only") == 0;
    int vertical_scroll_only = strcmp(mode, "vertical-scroll-only") == 0;
    int horizontal_scroll_only = strcmp(mode, "horizontal-scroll-only") == 0;
    int async_input = strcmp(mode, "async-input") == 0;
    int click_only = strcmp(mode, "click-only") == 0;
    int move_only = strcmp(mode, "move-only") == 0;
    int button_only = strcmp(mode, "button-only") == 0;
    int any_scroll_only = scroll_only || vertical_scroll_only || horizontal_scroll_only;
    struct state state = {0};
    struct wl_display *display = wl_display_connect(NULL);
    if (!display) {
        fprintf(stderr, "connect failed: %s\n", strerror(errno));
        return 2;
    }

    struct wl_registry *registry = wl_display_get_registry(display);
    wl_registry_add_listener(registry, &registry_listener, &state);
    wl_display_roundtrip(display);
    if (!state.test) {
        fprintf(stderr, "weston_test global missing\n");
        return 3;
    }

    uint32_t hi, lo, ns;
    wl_display_roundtrip(display);
    uint64_t pointer_move_monotonic_ns = 0;
    if (!button_only) {
        stamp(&hi, &lo, &ns);
        pointer_move_monotonic_ns = monotonic_nsec();
        weston_test_move_pointer(state.test, hi, lo, ns, x, y);
        wl_display_roundtrip(display);
    }

    uint64_t button_press_monotonic_ns = 0;
    uint64_t button_release_monotonic_ns = 0;
    int sent_scroll_axis_event_count = 0;
    int sent_scroll_burst_count = 0;
    uint64_t first_scroll_monotonic_ns = 0;
    uint64_t last_scroll_monotonic_ns = 0;
    if (!any_scroll_only && !move_only) {
        stamp(&hi, &lo, &ns);
        button_press_monotonic_ns = monotonic_nsec();
        weston_test_send_button(state.test, hi, lo, ns, BTN_LEFT, WL_POINTER_BUTTON_STATE_PRESSED);
        if (!async_input && !button_only)
            wl_display_roundtrip(display);
        stamp(&hi, &lo, &ns);
        button_release_monotonic_ns = monotonic_nsec();
        weston_test_send_button(state.test, hi, lo, ns, BTN_LEFT, WL_POINTER_BUTTON_STATE_RELEASED);
        if (async_input || button_only) {
            wl_display_flush(display);
            wl_display_roundtrip(display);
        } else {
            wl_display_roundtrip(display);
        }
    }

    if (!async_input && !click_only && !move_only && !button_only) {
        for (int repeat = 0; repeat < repeat_count; repeat++) {
            stamp(&hi, &lo, &ns);
            uint64_t scroll_monotonic_ns = monotonic_nsec();
            if (first_scroll_monotonic_ns == 0)
                first_scroll_monotonic_ns = scroll_monotonic_ns;
            last_scroll_monotonic_ns = scroll_monotonic_ns;
            if (!horizontal_scroll_only) {
                weston_test_send_axis(
                    state.test,
                    hi,
                    lo,
                    ns,
                    WL_POINTER_AXIS_VERTICAL_SCROLL,
                    wl_fixed_from_double(120.0));
                sent_scroll_axis_event_count++;
            }
            stamp(&hi, &lo, &ns);
            if (!vertical_scroll_only) {
                weston_test_send_axis(
                    state.test,
                    hi,
                    lo,
                    ns,
                    WL_POINTER_AXIS_HORIZONTAL_SCROLL,
                    wl_fixed_from_double(90.0));
                sent_scroll_axis_event_count++;
            }
            sent_scroll_burst_count++;
            wl_display_roundtrip(display);
            if (repeat + 1 < repeat_count)
                sleep_millis(repeat_delay_ms);
        }
    }

    if (text && !any_scroll_only && !move_only && !button_only) {
        for (const char *cursor = text; *cursor; cursor++) {
            int key = key_code_for_char(*cursor);
            if (key)
                send_key_press(state.test, key);
        }
        if (send_enter)
            send_key_press(state.test, KEY_ENTER);
    } else if (!any_scroll_only && !click_only && !move_only && !button_only) {
        send_key_press(state.test, KEY_A);
    }
    if (async_input)
        wl_display_flush(display);
    else
        wl_display_roundtrip(display);

    fprintf(
        stdout,
        "{\"status\":\"pass\",\"x\":%d,\"y\":%d,\"pointer_events\":%u,"
        "\"last_pointer_x\":%d,\"last_pointer_y\":%d,\"typed_text\":\"%s\","
        "\"sent_enter\":%s,\"scroll_only\":%s,\"scroll_mode\":\"%s\",\"async_input\":%s,"
        "\"repeat_count\":%d,\"repeat_delay_ms\":%d,"
        "\"sent_scroll_axis_event_count\":%d,\"sent_scroll_burst_count\":%d,"
        "\"process_start_monotonic_ns\":%" PRIu64 ","
        "\"pointer_move_monotonic_ns\":%" PRIu64 ","
        "\"first_scroll_monotonic_ns\":%" PRIu64 ","
        "\"last_scroll_monotonic_ns\":%" PRIu64 ","
        "\"button_press_monotonic_ns\":%" PRIu64 ","
        "\"button_release_monotonic_ns\":%" PRIu64 "}\n",
        x,
        y,
        state.pointer_events,
        state.pointer_x,
        state.pointer_y,
        text ? text : "",
        send_enter ? "true" : "false",
        any_scroll_only ? "true" : "false",
        mode,
        async_input ? "true" : "false",
        repeat_count,
        repeat_delay_ms,
        sent_scroll_axis_event_count,
        sent_scroll_burst_count,
        process_start_monotonic_ns,
        pointer_move_monotonic_ns,
        first_scroll_monotonic_ns,
        last_scroll_monotonic_ns,
        button_press_monotonic_ns,
        button_release_monotonic_ns);

    weston_test_destroy(state.test);
    wl_registry_destroy(registry);
    wl_display_disconnect(display);
    return 0;
}

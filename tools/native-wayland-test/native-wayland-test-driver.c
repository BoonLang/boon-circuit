#include <ctype.h>
#include <errno.h>
#include <limits.h>
#include <linux/input-event-codes.h>
#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <wayland-client.h>

#include "weston-test-client-protocol.h"

struct control {
    struct wl_display *display;
    struct wl_registry *registry;
    struct weston_test *test;
    uint32_t advertised_version;
    wl_fixed_t pointer_x;
    wl_fixed_t pointer_y;
    int pointer_seen;
};

struct name_code {
    const char *name;
    uint32_t code;
};

static const struct name_code key_names[] = {
    {"a", KEY_A}, {"b", KEY_B}, {"c", KEY_C}, {"d", KEY_D},
    {"e", KEY_E}, {"f", KEY_F}, {"g", KEY_G}, {"h", KEY_H},
    {"i", KEY_I}, {"j", KEY_J}, {"k", KEY_K}, {"l", KEY_L},
    {"m", KEY_M}, {"n", KEY_N}, {"o", KEY_O}, {"p", KEY_P},
    {"q", KEY_Q}, {"r", KEY_R}, {"s", KEY_S}, {"t", KEY_T},
    {"u", KEY_U}, {"v", KEY_V}, {"w", KEY_W}, {"x", KEY_X},
    {"y", KEY_Y}, {"z", KEY_Z}, {"0", KEY_0}, {"1", KEY_1},
    {"2", KEY_2}, {"3", KEY_3}, {"4", KEY_4}, {"5", KEY_5},
    {"6", KEY_6}, {"7", KEY_7}, {"8", KEY_8}, {"9", KEY_9},
    {"enter", KEY_ENTER}, {"escape", KEY_ESC}, {"tab", KEY_TAB},
    {"space", KEY_SPACE}, {"backspace", KEY_BACKSPACE},
    {"delete", KEY_DELETE}, {"left", KEY_LEFT}, {"right", KEY_RIGHT},
    {"up", KEY_UP}, {"down", KEY_DOWN},
};

static void pointer_position(void *data, struct weston_test *test,
                             wl_fixed_t x, wl_fixed_t y)
{
    struct control *control = data;

    (void)test;
    control->pointer_x = x;
    control->pointer_y = y;
    control->pointer_seen = 1;
}

static const struct weston_test_listener test_listener = {
    .pointer_position = pointer_position,
};

static void registry_global(void *data, struct wl_registry *registry,
                            uint32_t name, const char *interface,
                            uint32_t version)
{
    struct control *control = data;

    if (!control->test && strcmp(interface, "weston_test") == 0) {
        control->advertised_version = version;
        control->test = wl_registry_bind(registry, name,
                                         &weston_test_interface,
                                         version < 1 ? version : 1);
        weston_test_add_listener(control->test, &test_listener, control);
    }
}

static void registry_global_remove(void *data, struct wl_registry *registry,
                                   uint32_t name)
{
    (void)data;
    (void)registry;
    (void)name;
}

static const struct wl_registry_listener registry_listener = {
    .global = registry_global,
    .global_remove = registry_global_remove,
};

static int connect_control(struct control *control)
{
    memset(control, 0, sizeof(*control));
    control->display = wl_display_connect(NULL);
    if (!control->display) {
        fprintf(stderr, "cannot connect to Wayland display: %s\n",
                strerror(errno));
        return -1;
    }

    control->registry = wl_display_get_registry(control->display);
    wl_registry_add_listener(control->registry, &registry_listener, control);
    if (wl_display_roundtrip(control->display) < 0) {
        fprintf(stderr, "Wayland registry roundtrip failed\n");
        return -1;
    }
    if (!control->test) {
        fprintf(stderr, "weston_test v1 is not advertised\n");
        return -1;
    }
    if (wl_display_roundtrip(control->display) < 0) {
        fprintf(stderr, "weston_test bind roundtrip failed\n");
        return -1;
    }
    return 0;
}

static void disconnect_control(struct control *control)
{
    if (control->test)
        weston_test_destroy(control->test);
    if (control->registry)
        wl_registry_destroy(control->registry);
    if (control->display)
        wl_display_disconnect(control->display);
}

static int sync_control(struct control *control)
{
    if (wl_display_roundtrip(control->display) >= 0)
        return 0;
    fprintf(stderr, "weston_test request failed: Wayland error %d\n",
            wl_display_get_error(control->display));
    return -1;
}

static void stamp(uint32_t *sec_hi, uint32_t *sec_lo, uint32_t *nsec)
{
    struct timespec now;
    uint64_t seconds;

    clock_gettime(CLOCK_MONOTONIC, &now);
    seconds = (uint64_t)now.tv_sec;
    *sec_hi = (uint32_t)(seconds >> 32);
    *sec_lo = (uint32_t)seconds;
    *nsec = (uint32_t)now.tv_nsec;
}

static int parse_long(const char *text, long minimum, long maximum, long *value)
{
    char *end;
    long parsed;

    errno = 0;
    parsed = strtol(text, &end, 0);
    if (errno || end == text || *end != '\0' || parsed < minimum ||
        parsed > maximum)
        return -1;
    *value = parsed;
    return 0;
}

static int parse_double(const char *text, double *value)
{
    char *end;
    double parsed;

    errno = 0;
    parsed = strtod(text, &end);
    if (errno || end == text || *end != '\0' || !isfinite(parsed) ||
        parsed < -8388608.0 || parsed > 8388607.0)
        return -1;
    *value = parsed;
    return 0;
}

static int parse_button(const char *text, int32_t *button)
{
    long parsed;

    if (strcmp(text, "left") == 0)
        *button = BTN_LEFT;
    else if (strcmp(text, "middle") == 0)
        *button = BTN_MIDDLE;
    else if (strcmp(text, "right") == 0)
        *button = BTN_RIGHT;
    else if (parse_long(text, 0, INT32_MAX, &parsed) == 0)
        *button = (int32_t)parsed;
    else
        return -1;
    return 0;
}

static int parse_axis(const char *text, uint32_t *axis)
{
    if (strcmp(text, "vertical") == 0)
        *axis = WL_POINTER_AXIS_VERTICAL_SCROLL;
    else if (strcmp(text, "horizontal") == 0)
        *axis = WL_POINTER_AXIS_HORIZONTAL_SCROLL;
    else
        return -1;
    return 0;
}

static int parse_key(const char *text, uint32_t *key)
{
    long parsed;
    size_t i;
    char normalized[2];

    if (text[0] && !text[1] && isalpha((unsigned char)text[0])) {
        normalized[0] = (char)tolower((unsigned char)text[0]);
        normalized[1] = '\0';
        text = normalized;
    }
    for (i = 0; i < sizeof(key_names) / sizeof(key_names[0]); ++i) {
        if (strcmp(text, key_names[i].name) == 0) {
            *key = key_names[i].code;
            return 0;
        }
    }
    if (parse_long(text, 0, UINT32_MAX, &parsed) < 0)
        return -1;
    *key = (uint32_t)parsed;
    return 0;
}

static void send_button(struct control *control, int32_t button,
                        uint32_t state)
{
    uint32_t sec_hi, sec_lo, nsec;

    stamp(&sec_hi, &sec_lo, &nsec);
    weston_test_send_button(control->test, sec_hi, sec_lo, nsec, button,
                            state);
}

static void send_axis(struct control *control, uint32_t axis, double value)
{
    uint32_t sec_hi, sec_lo, nsec;

    stamp(&sec_hi, &sec_lo, &nsec);
    weston_test_send_axis(control->test, sec_hi, sec_lo, nsec, axis,
                          wl_fixed_from_double(value));
}

static void send_key(struct control *control, uint32_t key, uint32_t state)
{
    uint32_t sec_hi, sec_lo, nsec;

    stamp(&sec_hi, &sec_lo, &nsec);
    weston_test_send_key(control->test, sec_hi, sec_lo, nsec, key, state);
}

static void usage(FILE *stream, const char *program)
{
    fprintf(stream,
            "usage:\n"
            "  %s query\n"
            "  %s move X Y\n"
            "  %s button down|up [left|middle|right|CODE]\n"
            "  %s click [left|middle|right|CODE]\n"
            "  %s axis vertical|horizontal VALUE\n"
            "  %s wheel vertical|horizontal TICKS [DISTANCE]\n"
            "  %s key down|up|press KEY\n"
            "  %s chord MODIFIER KEY\n"
            "  %s deactivate\n",
            program, program, program, program, program, program, program,
            program, program);
}

static int run_command(struct control *control, int argc, char **argv)
{
    const char *command = argv[1];

    if (strcmp(command, "query") == 0 && argc == 2) {
        printf("weston_test=v1 advertised_version=%u pointer=%s x=%.3f y=%.3f\n",
               control->advertised_version,
               control->pointer_seen ? "available" : "unavailable",
               wl_fixed_to_double(control->pointer_x),
               wl_fixed_to_double(control->pointer_y));
        return control->pointer_seen ? 0 : -1;
    }

    if (strcmp(command, "move") == 0 && argc == 4) {
        long x, y;
        uint32_t sec_hi, sec_lo, nsec;

        if (parse_long(argv[2], INT32_MIN, INT32_MAX, &x) < 0 ||
            parse_long(argv[3], INT32_MIN, INT32_MAX, &y) < 0)
            return -1;
        stamp(&sec_hi, &sec_lo, &nsec);
        weston_test_move_pointer(control->test, sec_hi, sec_lo, nsec,
                                 (int32_t)x, (int32_t)y);
        if (sync_control(control) < 0)
            return -1;
        printf("ok move x=%.3f y=%.3f\n",
               wl_fixed_to_double(control->pointer_x),
               wl_fixed_to_double(control->pointer_y));
        return 0;
    }

    if (strcmp(command, "button") == 0 && (argc == 3 || argc == 4)) {
        int32_t button;
        uint32_t state;

        if (parse_button(argc == 4 ? argv[3] : "left", &button) < 0)
            return -1;
        if (strcmp(argv[2], "down") == 0)
            state = WL_POINTER_BUTTON_STATE_PRESSED;
        else if (strcmp(argv[2], "up") == 0)
            state = WL_POINTER_BUTTON_STATE_RELEASED;
        else
            return -1;
        send_button(control, button, state);
        if (sync_control(control) < 0)
            return -1;
        printf("ok button code=%d state=%s\n", button, argv[2]);
        return 0;
    }

    if (strcmp(command, "click") == 0 && (argc == 2 || argc == 3)) {
        int32_t button;

        if (parse_button(argc == 3 ? argv[2] : "left", &button) < 0)
            return -1;
        send_button(control, button, WL_POINTER_BUTTON_STATE_PRESSED);
        if (sync_control(control) < 0)
            return -1;
        send_button(control, button, WL_POINTER_BUTTON_STATE_RELEASED);
        if (sync_control(control) < 0)
            return -1;
        printf("ok click code=%d\n", button);
        return 0;
    }

    if (strcmp(command, "axis") == 0 && argc == 4) {
        uint32_t axis;
        double value;

        if (parse_axis(argv[2], &axis) < 0 ||
            parse_double(argv[3], &value) < 0)
            return -1;
        send_axis(control, axis, value);
        if (sync_control(control) < 0)
            return -1;
        printf("ok axis name=%s value=%.3f\n", argv[2], value);
        return 0;
    }

    if (strcmp(command, "wheel") == 0 && (argc == 4 || argc == 5)) {
        uint32_t axis;
        long ticks;
        long count, i;
        double distance = 120.0;
        double value;

        if (parse_axis(argv[2], &axis) < 0 ||
            parse_long(argv[3], -1000, 1000, &ticks) < 0 || ticks == 0 ||
            (argc == 5 && parse_double(argv[4], &distance) < 0) ||
            distance <= 0.0)
            return -1;
        count = ticks < 0 ? -ticks : ticks;
        value = ticks < 0 ? -distance : distance;
        for (i = 0; i < count; ++i) {
            send_axis(control, axis, value);
            if (sync_control(control) < 0)
                return -1;
        }
        printf("ok wheel name=%s ticks=%ld distance=%.3f\n", argv[2], ticks,
               distance);
        return 0;
    }

    if (strcmp(command, "key") == 0 && argc == 4) {
        uint32_t key;

        if (parse_key(argv[3], &key) < 0)
            return -1;
        if (strcmp(argv[2], "down") == 0) {
            send_key(control, key, WL_KEYBOARD_KEY_STATE_PRESSED);
        } else if (strcmp(argv[2], "up") == 0) {
            send_key(control, key, WL_KEYBOARD_KEY_STATE_RELEASED);
        } else if (strcmp(argv[2], "press") == 0) {
            send_key(control, key, WL_KEYBOARD_KEY_STATE_PRESSED);
            if (sync_control(control) < 0)
                return -1;
            send_key(control, key, WL_KEYBOARD_KEY_STATE_RELEASED);
        } else {
            return -1;
        }
        if (sync_control(control) < 0)
            return -1;
        printf("ok key code=%u state=%s\n", key, argv[2]);
        return 0;
    }

    if (strcmp(command, "chord") == 0 && argc == 4) {
        uint32_t modifier;
        uint32_t key;

        if (parse_key(argv[2], &modifier) < 0 ||
            parse_key(argv[3], &key) < 0)
            return -1;
        send_key(control, modifier, WL_KEYBOARD_KEY_STATE_PRESSED);
        send_key(control, key, WL_KEYBOARD_KEY_STATE_PRESSED);
        send_key(control, key, WL_KEYBOARD_KEY_STATE_RELEASED);
        send_key(control, modifier, WL_KEYBOARD_KEY_STATE_RELEASED);
        if (sync_control(control) < 0)
            return -1;
        printf("ok chord modifier=%u key=%u\n", modifier, key);
        return 0;
    }

    if (strcmp(command, "deactivate") == 0 && argc == 2) {
        weston_test_activate_surface(control->test, NULL);
        if (sync_control(control) < 0)
            return -1;
        printf("ok keyboard-focus=cleared\n");
        return 0;
    }

    return -1;
}

int main(int argc, char **argv)
{
    struct control control;
    int status;

    if (argc < 2) {
        usage(stderr, argv[0]);
        return 2;
    }
    if (strcmp(argv[1], "help") == 0) {
        usage(stdout, argv[0]);
        return 0;
    }
    if (connect_control(&control) < 0) {
        disconnect_control(&control);
        return 3;
    }

    status = run_command(&control, argc, argv);
    if (status < 0) {
        usage(stderr, argv[0]);
        status = 2;
    }
    disconnect_control(&control);
    return status;
}

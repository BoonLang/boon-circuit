/*
 * Derived from Weston's tests/weston-test.c, Copyright 2012 Intel.
 * SPDX-License-Identifier: MIT
 */

#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>
#include <time.h>
#include <wayland-server-core.h>
#include <wayland-server-protocol.h>
#include <libweston/libweston.h>
#include <weston/weston.h>

#include "weston-test-server-protocol.h"

/* Exported by libweston-13 but intentionally absent from installed headers. */
void weston_seat_init(struct weston_seat *, struct weston_compositor *,
                      const char *);
void weston_seat_release(struct weston_seat *);
int weston_seat_init_pointer(struct weston_seat *);
int weston_seat_init_keyboard(struct weston_seat *, struct xkb_keymap *);
void notify_motion(struct weston_seat *, const struct timespec *,
                   struct weston_pointer_motion_event *);
void notify_button(struct weston_seat *, const struct timespec *, int32_t,
                   enum wl_pointer_button_state);
void notify_axis(struct weston_seat *, const struct timespec *,
                 struct weston_pointer_axis_event *);
void notify_key(struct weston_seat *, const struct timespec *, uint32_t,
                enum wl_keyboard_key_state, enum weston_key_state_update);

struct test_control {
    struct weston_compositor *compositor;
    struct weston_seat owned_seat;
    struct weston_seat *seat;
    struct wl_listener destroy_listener;
    struct wl_client *bound_client;
    bool owns_seat;
};

static bool decode_time(struct wl_client *client, uint32_t sec_hi,
                        uint32_t sec_lo, uint32_t nsec, struct timespec *time)
{
    uint64_t seconds;

    if (nsec >= 1000000000U) {
        wl_client_post_implementation_error(client,
                                            "weston_test: invalid timestamp");
        return false;
    }

    seconds = ((uint64_t)sec_hi << 32) | sec_lo;
    time->tv_sec = (time_t)seconds;
    time->tv_nsec = (long)nsec;
    return true;
}

static void send_pointer_position(struct test_control *control,
                                  struct wl_resource *resource)
{
    struct weston_pointer *pointer = weston_seat_get_pointer(control->seat);

    weston_test_send_pointer_position(
        resource, wl_fixed_from_double(pointer->pos.c.x),
        wl_fixed_from_double(pointer->pos.c.y));
}

static void unsupported(struct wl_client *client, const char *request)
{
    wl_client_post_implementation_error(client,
                                        "weston_test.%s is not supported",
                                        request);
}

static void handle_move_surface(struct wl_client *client,
                                struct wl_resource *resource,
                                struct wl_resource *surface, int32_t x,
                                int32_t y)
{
    (void)resource;
    (void)surface;
    (void)x;
    (void)y;
    unsupported(client, "move_surface");
}

static void handle_move_pointer(struct wl_client *client,
                                struct wl_resource *resource,
                                uint32_t sec_hi, uint32_t sec_lo,
                                uint32_t nsec, int32_t x, int32_t y)
{
    struct test_control *control = wl_resource_get_user_data(resource);
    struct weston_pointer *pointer = weston_seat_get_pointer(control->seat);
    struct weston_coord_global position;
    struct weston_pointer_motion_event event = {0};
    struct timespec time;

    if (!decode_time(client, sec_hi, sec_lo, nsec, &time))
        return;

    position.c = weston_coord(x, y);
    event.mask = WESTON_POINTER_MOTION_REL;
    event.rel = weston_coord_global_sub(position, pointer->pos).c;
    notify_motion(control->seat, &time, &event);
    send_pointer_position(control, resource);
}

static void handle_send_button(struct wl_client *client,
                               struct wl_resource *resource,
                               uint32_t sec_hi, uint32_t sec_lo,
                               uint32_t nsec, int32_t button, uint32_t state)
{
    struct test_control *control = wl_resource_get_user_data(resource);
    struct timespec time;

    if (state > WL_POINTER_BUTTON_STATE_PRESSED) {
        wl_client_post_implementation_error(client,
                                            "weston_test: invalid button state");
        return;
    }
    if (!decode_time(client, sec_hi, sec_lo, nsec, &time))
        return;

    notify_button(control->seat, &time, button,
                  (enum wl_pointer_button_state)state);
}

static void handle_send_axis(struct wl_client *client,
                             struct wl_resource *resource,
                             uint32_t sec_hi, uint32_t sec_lo, uint32_t nsec,
                             uint32_t axis, wl_fixed_t value)
{
    struct test_control *control = wl_resource_get_user_data(resource);
    struct weston_pointer_axis_event event = {0};
    struct timespec time;

    if (axis > WL_POINTER_AXIS_HORIZONTAL_SCROLL) {
        wl_client_post_implementation_error(client,
                                            "weston_test: invalid pointer axis");
        return;
    }
    if (!decode_time(client, sec_hi, sec_lo, nsec, &time))
        return;

    event.axis = axis;
    event.value = wl_fixed_to_double(value);
    notify_axis(control->seat, &time, &event);
}

static void handle_activate_surface(struct wl_client *client,
                                    struct wl_resource *resource,
                                    struct wl_resource *surface_resource)
{
    struct test_control *control = wl_resource_get_user_data(resource);
    struct weston_surface *surface = NULL;

    (void)client;
    if (surface_resource)
        surface = wl_resource_get_user_data(surface_resource);
    weston_seat_set_keyboard_focus(control->seat, surface);
}

static void handle_send_key(struct wl_client *client,
                            struct wl_resource *resource, uint32_t sec_hi,
                            uint32_t sec_lo, uint32_t nsec, uint32_t key,
                            uint32_t state)
{
    struct test_control *control = wl_resource_get_user_data(resource);
    struct timespec time;

    if (state > WL_KEYBOARD_KEY_STATE_PRESSED) {
        wl_client_post_implementation_error(client,
                                            "weston_test: invalid key state");
        return;
    }
    if (!decode_time(client, sec_hi, sec_lo, nsec, &time))
        return;

    notify_key(control->seat, &time, key,
               (enum wl_keyboard_key_state)state, STATE_UPDATE_AUTOMATIC);
}

static void handle_device_release(struct wl_client *client,
                                  struct wl_resource *resource,
                                  const char *device)
{
    (void)resource;
    (void)device;
    unsupported(client, "device_release");
}

static void handle_device_add(struct wl_client *client,
                              struct wl_resource *resource,
                              const char *device)
{
    (void)resource;
    (void)device;
    unsupported(client, "device_add");
}

static void handle_send_touch(struct wl_client *client,
                              struct wl_resource *resource,
                              uint32_t sec_hi, uint32_t sec_lo, uint32_t nsec,
                              int32_t touch_id, wl_fixed_t x, wl_fixed_t y,
                              uint32_t touch_type)
{
    (void)resource;
    (void)sec_hi;
    (void)sec_lo;
    (void)nsec;
    (void)touch_id;
    (void)x;
    (void)y;
    (void)touch_type;
    unsupported(client, "send_touch");
}

static void handle_client_break(struct wl_client *client,
                                struct wl_resource *resource,
                                uint32_t breakpoint, uint32_t resource_id)
{
    (void)resource;
    (void)breakpoint;
    (void)resource_id;
    unsupported(client, "client_break");
}

static const struct weston_test_interface test_implementation = {
    .move_surface = handle_move_surface,
    .move_pointer = handle_move_pointer,
    .send_button = handle_send_button,
    .send_axis = handle_send_axis,
    .activate_surface = handle_activate_surface,
    .send_key = handle_send_key,
    .device_release = handle_device_release,
    .device_add = handle_device_add,
    .send_touch = handle_send_touch,
    .client_break = handle_client_break,
};

static void destroy_control_resource(struct wl_resource *resource)
{
    struct test_control *control = wl_resource_get_user_data(resource);

    if (control)
        control->bound_client = NULL;
}

static void bind_test(struct wl_client *client, void *data, uint32_t version,
                      uint32_t id)
{
    struct test_control *control = data;
    struct wl_resource *resource;

    if (control->bound_client) {
        wl_client_post_implementation_error(client,
                                            "weston_test is already bound");
        return;
    }

    resource = wl_resource_create(client, &weston_test_interface,
                                  version < 1 ? version : 1, id);
    if (!resource) {
        wl_client_post_no_memory(client);
        return;
    }

    control->bound_client = client;
    wl_resource_set_implementation(resource, &test_implementation, control,
                                   destroy_control_resource);
    send_pointer_position(control, resource);
}

static void handle_compositor_destroy(struct wl_listener *listener, void *data)
{
    struct test_control *control =
        wl_container_of(listener, control, destroy_listener);

    (void)data;
    wl_list_remove(&control->destroy_listener.link);
    if (control->owns_seat)
        weston_seat_release(&control->owned_seat);
    free(control);
}

WL_EXPORT int wet_module_init(struct weston_compositor *compositor, int *argc,
                              char *argv[])
{
    struct test_control *control;
    struct weston_seat *seat;
    int major, minor, micro;

    (void)argc;
    (void)argv;
    weston_version(&major, &minor, &micro);
    if (major != 13 || minor != 0 || micro != 0) {
        weston_log("native-wayland-test requires Weston 13.0.0, found %d.%d.%d\n",
                   major, minor, micro);
        return -1;
    }

    control = calloc(1, sizeof(*control));
    if (!control)
        return -1;
    control->compositor = compositor;
    control->destroy_listener.notify = handle_compositor_destroy;
    if (!weston_compositor_add_destroy_listener_once(
            compositor, &control->destroy_listener,
            handle_compositor_destroy)) {
        free(control);
        return 0;
    }

    if (!wl_list_empty(&compositor->seat_list)) {
        seat = wl_container_of(compositor->seat_list.next, seat, link);
        control->seat = seat;
        if (!weston_seat_get_pointer(seat) || !weston_seat_get_keyboard(seat))
            goto fail;
    } else {
        weston_seat_init(&control->owned_seat, compositor,
                         "native-wayland-test");
        control->seat = &control->owned_seat;
        control->owns_seat = true;
        if (weston_seat_init_pointer(control->seat) < 0 ||
            weston_seat_init_keyboard(control->seat, NULL) < 0)
            goto fail;
    }
    weston_log("native-wayland-test controls %s seat %s\n",
               control->owns_seat ? "owned" : "existing",
               control->seat->seat_name);

    if (!wl_global_create(compositor->wl_display, &weston_test_interface, 1,
                          control, bind_test))
        goto fail;
    return 0;

fail:
    wl_list_remove(&control->destroy_listener.link);
    if (control->owns_seat)
        weston_seat_release(&control->owned_seat);
    free(control);
    return -1;
}

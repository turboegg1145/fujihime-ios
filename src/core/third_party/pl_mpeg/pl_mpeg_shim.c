#include <stddef.h>
#include <stdint.h>
#include <stdio.h>

#define PL_MPEG_IMPLEMENTATION
#include "pl_mpeg.h"

plm_t *fujihime_plm_create_with_memory(const uint8_t *bytes, size_t length) {
    if (!bytes || length == 0) {
        return NULL;
    }
    return plm_create_with_memory((uint8_t *)bytes, length, 0);
}

void fujihime_plm_destroy(plm_t *plm) {
    if (plm) {
        plm_destroy(plm);
    }
}

int32_t fujihime_plm_has_headers(plm_t *plm) {
    return plm ? (int32_t)plm_has_headers(plm) : 0;
}

int32_t fujihime_plm_get_width(plm_t *plm) {
    return plm ? (int32_t)plm_get_width(plm) : 0;
}

int32_t fujihime_plm_get_height(plm_t *plm) {
    return plm ? (int32_t)plm_get_height(plm) : 0;
}

double fujihime_plm_get_framerate(plm_t *plm) {
    return plm ? plm_get_framerate(plm) : 0.0;
}

int32_t fujihime_plm_get_samplerate(plm_t *plm) {
    return plm ? (int32_t)plm_get_samplerate(plm) : 0;
}

int32_t fujihime_plm_get_num_video_streams(plm_t *plm) {
    return plm ? (int32_t)plm_get_num_video_streams(plm) : 0;
}

int32_t fujihime_plm_get_num_audio_streams(plm_t *plm) {
    return plm ? (int32_t)plm_get_num_audio_streams(plm) : 0;
}

int32_t fujihime_plm_has_ended(plm_t *plm) {
    return plm ? (int32_t)plm_has_ended(plm) : 1;
}

void fujihime_plm_set_video_enabled(plm_t *plm, int32_t enabled) {
    if (plm) {
        plm_set_video_enabled(plm, enabled ? 1 : 0);
    }
}

void fujihime_plm_set_audio_enabled(plm_t *plm, int32_t enabled) {
    if (plm) {
        plm_set_audio_enabled(plm, enabled ? 1 : 0);
    }
}

void fujihime_plm_set_loop(plm_t *plm, int32_t loop) {
    if (plm) {
        plm_set_loop(plm, loop ? 1 : 0);
    }
}

void fujihime_plm_rewind(plm_t *plm) {
    if (plm) {
        plm_rewind(plm);
    }
}

int32_t fujihime_plm_decode_video_bgra(
    plm_t *plm,
    uint8_t *out_bgra,
    int32_t stride_bytes,
    double *out_time
) {
    if (!plm || !out_bgra) {
        return 0;
    }
    plm_frame_t *frame = plm_decode_video(plm);
    if (!frame) {
        return 0;
    }
    plm_frame_to_bgra(frame, out_bgra, stride_bytes);
    if (out_time) {
        *out_time = frame->time;
    }
    return 1;
}

uint32_t fujihime_plm_decode_audio(
    plm_t *plm,
    float *out_interleaved,
    uint32_t max_frames,
    double *out_time
) {
    if (!plm || !out_interleaved || max_frames < PLM_AUDIO_SAMPLES_PER_FRAME) {
        return 0;
    }
    plm_samples_t *samples = plm_decode_audio(plm);
    if (!samples) {
        return 0;
    }
    uint32_t count = samples->count;
    if (count > max_frames) {
        count = max_frames;
    }
    for (uint32_t i = 0; i < count * 2; i++) {
        out_interleaved[i] = samples->interleaved[i];
    }
    if (out_time) {
        *out_time = samples->time;
    }
    return count;
}

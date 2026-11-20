#include <stddef.h>
#include <stdint.h>
#include <string.h>

#include <ft2build.h>
#include FT_FREETYPE_H
#include FT_SYNTHESIS_H

typedef struct FujihimeFtGlyphOut {
    uint32_t width;
    uint32_t rows;
    int32_t pitch;
    uint8_t pixel_mode; /* 1 = mono (1-bit MSB), 2 = gray (8-bit) */
    int32_t bitmap_left;
    int32_t bitmap_top;
    int32_t advance_x_26_6;
    int32_t advance_y_26_6;
    int32_t hori_advance_26_6;
    int32_t vert_advance_26_6;
    int32_t ascender_px;
    int32_t descender_px;
    int32_t height_px;
    const uint8_t *buffer;
} FujihimeFtGlyphOut;

int32_t fujihime_ft_init(FT_Library *out_lib) {
    if (!out_lib) {
        return -1;
    }
    return (int32_t)FT_Init_FreeType(out_lib);
}

void fujihime_ft_done(FT_Library lib) {
    if (lib) {
        FT_Done_FreeType(lib);
    }
}

int32_t fujihime_ft_new_memory_face(
    FT_Library lib,
    const uint8_t *font_bytes,
    uint32_t font_len,
    int32_t face_index,
    FT_Face *out_face
) {
    if (!lib || !font_bytes || font_len == 0 || !out_face) {
        return -1;
    }
    FT_Error err = FT_New_Memory_Face(
        lib,
        (const FT_Byte *)font_bytes,
        (FT_Long)font_len,
        (FT_Long)face_index,
        out_face
    );
    if (err != 0) {
        return (int32_t)err;
    }
    /* Prefer Unicode charmap when present. */
    (void)FT_Select_Charmap(*out_face, FT_ENCODING_UNICODE);
    return 0;
}

void fujihime_ft_done_face(FT_Face face) {
    if (face) {
        FT_Done_Face(face);
    }
}

int32_t fujihime_ft_render_char(
    FT_Face face,
    uint32_t codepoint,
    uint32_t pixel_height,
    int32_t bold,
    int32_t italic,
    int32_t mono,
    int32_t vertical_ccw90,
    FujihimeFtGlyphOut *out_glyph
) {
    if (!face || !out_glyph || pixel_height == 0) {
        return -1;
    }
    memset(out_glyph, 0, sizeof(*out_glyph));

    FT_Error err = FT_Set_Pixel_Sizes(face, 0, (FT_UInt)pixel_height);
    if (err != 0) {
        return (int32_t)err;
    }

    FT_UInt glyph_index = FT_Get_Char_Index(face, (FT_ULong)codepoint);

    /* Apply optional 90-degree CCW rotation (matching GDI MAT2 {0,-1,1,0}). */
    if (vertical_ccw90) {
        FT_Matrix mat;
        mat.xx = 0;
        mat.xy = -0x10000L;
        mat.yx = 0x10000L;
        mat.yy = 0;
        FT_Set_Transform(face, &mat, NULL);
    } else {
        FT_Set_Transform(face, NULL, NULL);
    }

    FT_Int32 load_flags = FT_LOAD_NO_BITMAP;
    if (mono) {
        load_flags |= FT_LOAD_TARGET_MONO;
    } else {
        load_flags |= FT_LOAD_TARGET_NORMAL;
    }

    err = FT_Load_Glyph(face, glyph_index, load_flags);
    if (err != 0) {
        FT_Set_Transform(face, NULL, NULL);
        return (int32_t)err;
    }

    FT_GlyphSlot slot = face->glyph;
    if (bold) {
        FT_GlyphSlot_Embolden(slot);
    }
    if (italic) {
        FT_GlyphSlot_Oblique(slot);
    }

    FT_Render_Mode render_mode = mono ? FT_RENDER_MODE_MONO : FT_RENDER_MODE_NORMAL;
    err = FT_Render_Glyph(slot, render_mode);
    FT_Set_Transform(face, NULL, NULL);
    if (err != 0) {
        return (int32_t)err;
    }

    out_glyph->width = (uint32_t)slot->bitmap.width;
    out_glyph->rows = (uint32_t)slot->bitmap.rows;
    out_glyph->pitch = (int32_t)slot->bitmap.pitch;
    out_glyph->pixel_mode = (uint8_t)slot->bitmap.pixel_mode;
    out_glyph->bitmap_left = (int32_t)slot->bitmap_left;
    out_glyph->bitmap_top = (int32_t)slot->bitmap_top;
    out_glyph->advance_x_26_6 = (int32_t)slot->advance.x;
    out_glyph->advance_y_26_6 = (int32_t)slot->advance.y;
    out_glyph->hori_advance_26_6 = (int32_t)slot->metrics.horiAdvance;
    out_glyph->vert_advance_26_6 = (int32_t)slot->metrics.vertAdvance;
    out_glyph->ascender_px = (int32_t)(face->size->metrics.ascender >> 6);
    out_glyph->descender_px = (int32_t)(face->size->metrics.descender >> 6);
    out_glyph->height_px = (int32_t)(face->size->metrics.height >> 6);
    out_glyph->buffer = (const uint8_t *)slot->bitmap.buffer;
    return 0;
}

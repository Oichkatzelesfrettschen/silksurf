//! Canvas 2D context over the DOM-owned backing store.
//!
//! `document.createElement('canvas').getContext('2d')` returns a context whose
//! drawing calls mutate the canvas element's backing surface in the shared
//! `Dom`. These tests draw and then read the result back through
//! `getImageData`, so they verify real pixels without the page paint pipeline.

use std::sync::{Arc, Mutex};

use silksurf_dom::Dom;
use silksurf_js::SilkContext;

fn context_with_document() -> SilkContext {
    let mut dom = Dom::new();
    let document = dom.create_document();
    let html = dom.create_element("html");
    let body = dom.create_element("body");
    dom.append_child(document, html).expect("html attaches");
    dom.append_child(html, body).expect("body attaches");
    SilkContext::with_dom(&Arc::new(Mutex::new(dom)))
}

#[test]
fn get_context_returns_2d_for_canvas_and_null_otherwise() {
    let mut ctx = context_with_document();
    ctx.eval(
        "var canvas = document.createElement('canvas'); \
         var context = canvas.getContext('2d'); \
         if (!context) throw new Error('canvas getContext 2d returned null'); \
         if (typeof context.fillRect !== 'function') throw new Error('missing fillRect'); \
         var div = document.createElement('div'); \
         if (div.getContext('2d') !== null) throw new Error('div getContext should be null'); \
         if (canvas.getContext('webgl') !== null) throw new Error('unsupported type should be null');",
    )
    .expect("getContext contract holds");
}

#[test]
fn fill_rect_writes_pixels_readable_via_get_image_data() {
    let mut ctx = context_with_document();
    ctx.eval(
        "var canvas = document.createElement('canvas'); \
         canvas.setAttribute('width', '20'); \
         canvas.setAttribute('height', '20'); \
         var g = canvas.getContext('2d'); \
         g.fillStyle = '#ff0000'; \
         g.fillRect(4, 4, 8, 8); \
         var inside = g.getImageData(6, 6, 1, 1).data; \
         if (inside[0] !== 255 || inside[1] !== 0 || inside[2] !== 0 || inside[3] !== 255) \
             throw new Error('inside pixel ' + inside.join(',')); \
         var outside = g.getImageData(0, 0, 1, 1).data; \
         if (outside[3] !== 0) throw new Error('outside pixel alpha ' + outside[3]);",
    )
    .expect("fillRect pixels verified");
}

#[test]
fn rgba_fill_style_parses_and_blends() {
    let mut ctx = context_with_document();
    ctx.eval(
        "var canvas = document.createElement('canvas'); \
         canvas.setAttribute('width', '8'); \
         canvas.setAttribute('height', '8'); \
         var g = canvas.getContext('2d'); \
         g.fillStyle = 'rgb(255, 255, 255)'; \
         g.fillRect(0, 0, 8, 8); \
         g.fillStyle = 'rgba(255, 0, 0, 0.5)'; \
         g.fillRect(0, 0, 8, 8); \
         var px = g.getImageData(2, 2, 1, 1).data; \
         if (px[0] !== 255) throw new Error('red ' + px[0]); \
         if (Math.abs(px[1] - 127) > 2) throw new Error('green blend ' + px[1]); \
         if (px[3] !== 255) throw new Error('alpha ' + px[3]);",
    )
    .expect("rgba fillStyle parses and blends");
}

#[test]
fn clear_rect_restores_transparency() {
    let mut ctx = context_with_document();
    ctx.eval(
        "var canvas = document.createElement('canvas'); \
         canvas.setAttribute('width', '10'); \
         canvas.setAttribute('height', '10'); \
         var g = canvas.getContext('2d'); \
         g.fillStyle = '#0000ff'; \
         g.fillRect(0, 0, 10, 10); \
         g.clearRect(2, 2, 4, 4); \
         var cleared = g.getImageData(4, 4, 1, 1).data; \
         if (cleared[3] !== 0) throw new Error('cleared alpha ' + cleared[3]); \
         var kept = g.getImageData(0, 0, 1, 1).data; \
         if (kept[2] !== 255 || kept[3] !== 255) throw new Error('kept ' + kept.join(','));",
    )
    .expect("clearRect restores transparency");
}

#[test]
fn translate_offsets_drawing() {
    let mut ctx = context_with_document();
    ctx.eval(
        "var canvas = document.createElement('canvas'); \
         canvas.setAttribute('width', '16'); \
         canvas.setAttribute('height', '16'); \
         var g = canvas.getContext('2d'); \
         g.fillStyle = '#00ff00'; \
         g.translate(6, 6); \
         g.fillRect(0, 0, 3, 3); \
         var moved = g.getImageData(7, 7, 1, 1).data; \
         if (moved[1] !== 255) throw new Error('translated pixel ' + moved.join(',')); \
         var origin = g.getImageData(1, 1, 1, 1).data; \
         if (origin[3] !== 0) throw new Error('origin should be empty ' + origin.join(','));",
    )
    .expect("translate offsets drawing");
}

#[test]
fn path_triangle_fills_interior() {
    let mut ctx = context_with_document();
    ctx.eval(
        "var canvas = document.createElement('canvas'); \
         canvas.setAttribute('width', '16'); \
         canvas.setAttribute('height', '16'); \
         var g = canvas.getContext('2d'); \
         g.fillStyle = '#ffff00'; \
         g.beginPath(); \
         g.moveTo(2, 2); \
         g.lineTo(14, 2); \
         g.lineTo(2, 14); \
         g.closePath(); \
         g.fill(); \
         var inside = g.getImageData(4, 4, 1, 1).data; \
         if (inside[0] !== 255 || inside[1] !== 255) throw new Error('inside ' + inside.join(',')); \
         var outside = g.getImageData(13, 13, 1, 1).data; \
         if (outside[3] !== 0) throw new Error('outside ' + outside.join(','));",
    )
    .expect("path triangle fills");
}

#[test]
fn arc_fill_paints_disc_center() {
    let mut ctx = context_with_document();
    ctx.eval(
        "var canvas = document.createElement('canvas'); \
         canvas.setAttribute('width', '20'); \
         canvas.setAttribute('height', '20'); \
         var g = canvas.getContext('2d'); \
         g.fillStyle = '#123456'; \
         g.beginPath(); \
         g.arc(10, 10, 6, 0, Math.PI * 2, false); \
         g.closePath(); \
         g.fill(); \
         var center = g.getImageData(10, 10, 1, 1).data; \
         if (center[0] !== 0x12 || center[1] !== 0x34 || center[2] !== 0x56) \
             throw new Error('disc center ' + center.join(',')); \
         var corner = g.getImageData(0, 0, 1, 1).data; \
         if (corner[3] !== 0) throw new Error('disc corner ' + corner.join(','));",
    )
    .expect("arc fill paints a disc");
}

#[test]
fn put_and_get_image_data_round_trip() {
    let mut ctx = context_with_document();
    ctx.eval(
        "var canvas = document.createElement('canvas'); \
         canvas.setAttribute('width', '8'); \
         canvas.setAttribute('height', '8'); \
         var g = canvas.getContext('2d'); \
         var img = g.getImageData(0, 0, 2, 2); \
         for (var i = 0; i < img.data.length; i++) { img.data[i] = (i * 7) % 256; } \
         g.putImageData(img, 3, 3); \
         var read = g.getImageData(3, 3, 2, 2).data; \
         for (var j = 0; j < read.length; j++) { \
           if (read[j] !== (j * 7) % 256) throw new Error('mismatch at ' + j + ': ' + read[j]); \
         }",
    )
    .expect("putImageData/getImageData round-trips");
}

#[test]
fn draw_image_from_another_canvas_copies_pixels() {
    let mut ctx = context_with_document();
    ctx.eval(
        "var src = document.createElement('canvas'); \
         src.setAttribute('width', '4'); \
         src.setAttribute('height', '4'); \
         var sg = src.getContext('2d'); \
         sg.fillStyle = '#ff00ff'; \
         sg.fillRect(0, 0, 4, 4); \
         var dst = document.createElement('canvas'); \
         dst.setAttribute('width', '16'); \
         dst.setAttribute('height', '16'); \
         var dg = dst.getContext('2d'); \
         dg.drawImage(src, 2, 2, 8, 8); \
         var px = dg.getImageData(5, 5, 1, 1).data; \
         if (px[0] !== 255 || px[1] !== 0 || px[2] !== 255) throw new Error('drawn ' + px.join(',')); \
         var empty = dg.getImageData(0, 0, 1, 1).data; \
         if (empty[3] !== 0) throw new Error('outside draw ' + empty.join(','));",
    )
    .expect("drawImage copies from a source canvas");
}

#[test]
fn save_restore_isolates_fill_style() {
    let mut ctx = context_with_document();
    ctx.eval(
        "var canvas = document.createElement('canvas'); \
         canvas.setAttribute('width', '4'); \
         canvas.setAttribute('height', '4'); \
         var g = canvas.getContext('2d'); \
         g.fillStyle = '#ff0000'; \
         g.save(); \
         g.fillStyle = '#00ff00'; \
         g.restore(); \
         g.fillRect(0, 0, 4, 4); \
         var px = g.getImageData(1, 1, 1, 1).data; \
         if (px[0] !== 255 || px[1] !== 0) throw new Error('restored fill ' + px.join(','));",
    )
    .expect("save/restore isolates state");
}

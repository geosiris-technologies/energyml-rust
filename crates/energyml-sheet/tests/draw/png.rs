//! `draw::png`: the PNG of a view is its SVG rasterised, and nothing else. Each
//! test states the rule it checks and what would have to break for it to turn
//! red. The expected pixels come from the rule (a list drawn at a scale, a
//! raster stretched sharp over its rectangle, a later item over an earlier
//! one), never from a file.

#[cfg(not(feature = "png"))]
mod without_png {
    use energyml_sheet::draw::list::{Colour, DisplayList, PageUnit};
    use energyml_sheet::draw::png::{render, render_list};
    use energyml_sheet::draw::svg::{self, DrawError};

    /// ADR-0005; the rule of the section view's PNG. Casser : rendre une image
    /// vide au lieu du refus.
    #[test]
    fn png_refuses_by_name_without_its_feature() {
        let mut list = DisplayList::new([60.0, 30.0], PageUnit::Pixel);
        list.background = Some(Colour::rgb(255, 255, 255));
        assert!(matches!(
            render_list(&list, 1.0),
            Err(DrawError::FeatureNotEnabled { feature: "png", .. })
        ));
        let svg_text = svg::write(&list, None).unwrap();
        assert!(matches!(
            render(&svg_text, 1.0),
            Err(DrawError::FeatureNotEnabled { feature: "png", .. })
        ));
    }
}

#[cfg(feature = "png")]
mod with_png {
    use energyml_sheet::draw::list::{
        Anchor, Colour, DisplayList, Element, Item, Layer, PageUnit, Paint, Rect, TextStyle,
    };
    use energyml_sheet::draw::png::{render, render_list};
    use energyml_sheet::draw::raster::Raster;
    use energyml_sheet::draw::svg::{self, DrawError};
    use resvg::tiny_skia::Pixmap;
    use std::sync::Arc;

    const WHITE: Colour = Colour::rgb(255, 255, 255);

    fn plain_list() -> DisplayList {
        let mut list = DisplayList::new([60.0, 30.0], PageUnit::Pixel);
        list.background = Some(WHITE);
        list
    }

    const RED: [u8; 4] = [255, 0, 0, 255];
    const GREEN: [u8; 4] = [0, 255, 0, 255];
    const BLUE: [u8; 4] = [0, 0, 255, 255];
    const CLEAR: [u8; 4] = [0, 0, 0, 0];

    fn size_of(png: &[u8]) -> (u32, u32) {
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n", "not a PNG");
        (
            u32::from_be_bytes(png[16..20].try_into().unwrap()),
            u32::from_be_bytes(png[20..24].try_into().unwrap()),
        )
    }

    fn pixel(pixmap: &Pixmap, x: u32, y: u32) -> [u8; 4] {
        let p = pixmap.pixel(x, y).unwrap();
        [p.red(), p.green(), p.blue(), p.alpha()]
    }

    /// A 2 x 2 raster, `[red, green / blue, transparent]`, on a 40 x 40 page
    /// with a white background, stretched over `(10, 10, 20, 20)` in a layer
    /// named `image`, then a black 2 x 2 square over it in the same layer.
    fn raster_list(clip: Option<Rect>) -> DisplayList {
        let raster = Raster::from_rgba(2, 2, [RED, GREEN, BLUE, CLEAR].concat()).unwrap();
        let mut layer = Layer::new("image");
        layer.clip = clip;
        layer.elements.push(Element::new(vec![
            Item::Image {
                rect: Rect {
                    x: 10.0,
                    y: 10.0,
                    w: 20.0,
                    h: 20.0,
                },
                raster: Arc::new(raster),
            },
            Item::Rect {
                rect: Rect {
                    x: 12.0,
                    y: 12.0,
                    w: 2.0,
                    h: 2.0,
                },
                corner: 0.0,
                fill: Some(Paint::Solid(Colour::rgb(0, 0, 0))),
                stroke: None,
            },
        ]));
        let mut list = DisplayList::new([40.0, 40.0], PageUnit::Pixel);
        list.background = Some(WHITE);
        list.layers.push(layer);
        list
    }

    fn decode(list: &DisplayList) -> Pixmap {
        let png = render(&svg::write(list, None).unwrap(), 1.0).unwrap();
        Pixmap::decode_png(&png).unwrap()
    }

    /// Casser : ignorer `scale`.
    #[test]
    fn the_png_is_the_svg_at_the_scale_asked() {
        let list = plain_list();
        let svg_text = svg::write(&list, None).unwrap();
        assert_eq!(size_of(&render(&svg_text, 2.0).unwrap()), (120, 60));
        assert_eq!(size_of(&render(&svg_text, 1.0).unwrap()), (60, 30));
        // A scale that is not a positive number is 1.
        assert_eq!(size_of(&render(&svg_text, f32::NAN).unwrap()), (60, 30));
    }

    /// ADR-0015. Casser : (1) retirer `raster-images` de `Cargo.toml` (l'image
    /// n'est plus dessinée, `(15, 25)` devient blanc) ; (2) ne plus écrire
    /// `image-rendering` sur l'`<image>` (le lissage mêle rouge et vert en
    /// `(19, 15)`) ; (3) écrire les `<image>` à la fin de leur calque
    /// (`(13, 13)` devient rouge).
    #[test]
    fn the_png_of_a_list_with_a_raster_shows_its_pixels_in_their_place() {
        let pixmap = decode(&raster_list(None));
        assert_eq!(pixel(&pixmap, 15, 25), BLUE);
        assert_eq!(pixel(&pixmap, 25, 15), GREEN);
        assert_eq!(
            pixel(&pixmap, 25, 25),
            [255, 255, 255, 255],
            "the transparent pixel shows the background"
        );
        assert_eq!(
            pixel(&pixmap, 13, 13),
            [0, 0, 0, 255],
            "the square is drawn over the raster"
        );
        assert_eq!(pixel(&pixmap, 17, 17), RED);
        assert_eq!(pixel(&pixmap, 19, 15), RED, "sharp: no mix at the border");
        assert_eq!(pixel(&pixmap, 20, 15), GREEN, "sharp: no mix at the border");
    }

    /// Casser : ne pas écrire `clip-path` sur le groupe du calque.
    #[test]
    fn a_raster_is_clipped_by_its_layer() {
        let clip = Rect {
            x: 0.0,
            y: 0.0,
            w: 20.0,
            h: 40.0,
        };
        let pixmap = decode(&raster_list(Some(clip)));
        assert_eq!(pixel(&pixmap, 25, 15), [255, 255, 255, 255]);
        assert_eq!(pixel(&pixmap, 15, 15), RED);
    }

    /// Decision of 14/11 (Q11): a PNG is always the SVG rasterised. Casser :
    /// dans `render_list`, écrire la liste sans ses `Item::Image`, ou la
    /// dessiner autrement que par `svg::write`.
    #[test]
    fn the_png_of_a_list_is_the_png_of_its_svg() {
        let mut list = raster_list(None);
        list.layers[0].elements.push(Element::new(vec![Item::Text {
            at: [2.0, 8.0],
            text: "Rv".to_string(),
            style: TextStyle {
                size: 6.0,
                colour: Colour::rgb(0, 0, 0),
                anchor: Anchor::Start,
                rotate: 0.0,
            },
        }]));
        let meta = svg::Metadata {
            element: "test-view".to_string(),
            namespace: "urn:energyml:test-view".to_string(),
            format: 1,
            version: "0".to_string(),
            body: "{}".to_string(),
        };
        let of_list = render_list(&list, 2.0).unwrap();
        let of_svg = render(&svg::write(&list, None).unwrap(), 2.0).unwrap();
        let of_exported_svg = render(&svg::write(&list, Some(&meta)).unwrap(), 2.0).unwrap();
        assert_eq!(of_list, of_svg);
        assert_eq!(of_list, of_exported_svg, "the <metadata> is not drawn");

        // The comparison can see a missing raster.
        let mut without = list.clone();
        for layer in &mut without.layers {
            for element in &mut layer.elements {
                element.items.retain(|i| !matches!(i, Item::Image { .. }));
            }
        }
        assert_ne!(render_list(&without, 2.0).unwrap(), of_list);
    }

    fn svg_with_image(href: &str) -> String {
        format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" \
             width=\"10\" height=\"10\" viewBox=\"0 0 10 10\">\
             <image x=\"0\" y=\"0\" width=\"10\" height=\"10\" xlink:href=\"{href}\"/></svg>"
        )
    }

    /// ADR-0005: resvg would leave these images out without a word. Casser :
    /// rendre `Some(ImageKind::PNG(data))` sans contrôle (`render` rend `Ok`).
    #[test]
    fn only_an_embedded_png_is_drawn() {
        assert_eq!(
            render(&svg_with_image("data:image/jpeg;base64,/9j/"), 1.0),
            Err(DrawError::ImageNotPng {
                mime: "image/jpeg".to_string()
            })
        );
        assert_eq!(
            render(&svg_with_image("tile.png"), 1.0),
            Err(DrawError::ImageNotEmbedded {
                href: "tile.png".to_string()
            })
        );
        // "not a png", base64 (RFC 4648 section 4).
        assert!(matches!(
            render(&svg_with_image("data:image/png;base64,bm90IGEgcG5n"), 1.0),
            Err(DrawError::ImageUnreadable { .. })
        ));
    }

    /// RFC 4648 section 4, written here and not borrowed from `draw::svg`
    /// (two independent readings of one fact).
    fn base64(bytes: &[u8]) -> String {
        const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for chunk in bytes.chunks(3) {
            let n = chunk
                .iter()
                .enumerate()
                .fold(0u32, |n, (i, b)| n | (u32::from(*b) << (16 - 8 * i)));
            for k in 0..4 {
                if k <= chunk.len() {
                    out.push(ALPHABET[((n >> (18 - 6 * k)) & 63) as usize] as char);
                } else {
                    out.push('=');
                }
            }
        }
        out
    }

    /// Casser : retirer le contrôle de la taille de l'en-tête (le décodage
    /// échoue sur le CRC et le refus devient `ImageUnreadable`).
    #[test]
    fn an_embedded_png_too_large_is_refused_before_decoding() {
        let mut header = Vec::new();
        header.extend_from_slice(b"\x89PNG\r\n\x1a\n");
        header.extend_from_slice(&13u32.to_be_bytes());
        header.extend_from_slice(b"IHDR");
        header.extend_from_slice(&4097u32.to_be_bytes());
        header.extend_from_slice(&4096u32.to_be_bytes());
        header.extend_from_slice(&[8, 6, 0, 0, 0]);
        header.extend_from_slice(&0u32.to_be_bytes());
        let href = format!("data:image/png;base64,{}", base64(&header));
        assert_eq!(
            render(&svg_with_image(&href), 1.0),
            Err(DrawError::Size {
                width: 4097,
                height: 4096,
                limit: 16_777_216
            })
        );
    }

    /// Casser : ne pas refuser un SVG qui ne se lit pas.
    #[test]
    fn an_svg_that_does_not_parse_is_refused_by_name() {
        assert!(matches!(render("<svg", 1.0), Err(DrawError::Svg(_))));
    }
}

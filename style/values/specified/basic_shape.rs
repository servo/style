/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! CSS handling for the specified value of
//! [`basic-shape`][basic-shape]s
//!
//! [basic-shape]: https://drafts.csswg.org/css-shapes/#typedef-basic-shape

use crate::parser::{Parse, ParserContext};
use crate::values::computed::basic_shape::InsetRect as ComputedInsetRect;
use crate::values::computed::{Context, ToComputedValue};
use crate::values::generics::basic_shape as generic;
use crate::values::generics::basic_shape::{Path, PolygonCoord};
use crate::values::generics::rect::Rect;
use crate::values::specified::border::BorderRadius;
use crate::values::specified::image::Image;
use crate::values::specified::position::{Position, PositionOrAuto};
use crate::values::specified::url::SpecifiedUrl;
use crate::values::specified::{LengthPercentage, NonNegativeLengthPercentage, SVGPathData};
use crate::Zero;
use cssparser::Parser;
use std::fmt::{self, Write};
use style_traits::{CssWriter, ParseError, StyleParseErrorKind, ToCss};

/// A specified alias for FillRule.
pub use crate::values::generics::basic_shape::FillRule;

/// A specified `clip-path` value.
pub type ClipPath = generic::GenericClipPath<BasicShape, SpecifiedUrl>;

/// A specified `shape-outside` value.
pub type ShapeOutside = generic::GenericShapeOutside<BasicShape, Image>;

/// A specified basic shape.
pub type BasicShape = generic::GenericBasicShape<
    Position,
    LengthPercentage,
    NonNegativeLengthPercentage,
    BasicShapeRect,
>;

/// The specified value of `inset()`.
pub type InsetRect = generic::GenericInsetRect<LengthPercentage, NonNegativeLengthPercentage>;

/// A specified circle.
pub type Circle = generic::Circle<Position, NonNegativeLengthPercentage>;

/// A specified ellipse.
pub type Ellipse = generic::Ellipse<Position, NonNegativeLengthPercentage>;

/// The specified value of `ShapeRadius`.
pub type ShapeRadius = generic::ShapeRadius<NonNegativeLengthPercentage>;

/// The specified value of `Polygon`.
pub type Polygon = generic::GenericPolygon<LengthPercentage>;

/// The specified value of `xywh()`.
/// Defines a rectangle via offsets from the top and left edge of the reference box, and a
/// specified width and height.
///
/// The four <length-percentage>s define, respectively, the inset from the left edge of the
/// reference box, the inset from the top edge of the reference box, the width of the rectangle,
/// and the height of the rectangle.
///
/// https://drafts.csswg.org/css-shapes-1/#funcdef-basic-shape-xywh
#[derive(Clone, Debug, MallocSizeOf, PartialEq, SpecifiedValueInfo, ToShmem)]
pub struct Xywh {
    /// The left edge of the reference box.
    pub x: LengthPercentage,
    /// The top edge of the reference box.
    pub y: LengthPercentage,
    /// The specified width.
    pub width: NonNegativeLengthPercentage,
    /// The specified height.
    pub height: NonNegativeLengthPercentage,
    /// The optional <border-radius> argument(s) define rounded corners for the inset rectangle
    /// using the border-radius shorthand syntax.
    pub round: BorderRadius,
}

/// The specified value of <basic-shape-rect>.
/// <basic-shape-rect> = <inset()> | <rect()> | <xywh()>
///
/// https://drafts.csswg.org/css-shapes-1/#supported-basic-shapes
#[derive(Clone, Debug, MallocSizeOf, PartialEq, SpecifiedValueInfo, ToCss, ToShmem)]
pub enum BasicShapeRect {
    /// Defines an inset rectangle via insets from each edge of the reference box.
    Inset(InsetRect),
    /// Defines a xywh function.
    #[css(function)]
    Xywh(Xywh),
    // TODO: Bug 1786161. Add rect().
}

/// For filled shapes, we use fill-rule, and store it for path() and polygon().
/// For outline shapes, we should ignore fill-rule.
///
/// https://github.com/w3c/fxtf-drafts/issues/512
/// https://github.com/w3c/csswg-drafts/issues/7390
/// https://github.com/w3c/csswg-drafts/issues/3468
pub enum ShapeType {
    /// The CSS property uses filled shapes. The default behavior.
    Filled,
    /// The CSS property uses outline shapes. This is especially useful for offset-path.
    Outline,
}

/// The default `at <position>` if it is omitted.
///
/// https://github.com/w3c/csswg-drafts/issues/8695
///
/// FIXME: Bug 1837340. It seems we should always omit this component if the author doesn't specify
/// it. In order to avoid changing the behavior on the shipped clip-path and shape-outside, we
/// still use center as their default value for now.
pub enum DefaultPosition {
    /// Use standard default value, center, if "at <position>" is omitted.
    Center,
    /// The default value depends on the context. For example, offset-path:circle() may use the
    /// value of offset-position as its default position of the circle center. So we shouldn't
    /// assign a default value to its specified value and computed value. This makes the
    /// serialization ignore this component (and makes this value non-interpolated with other
    /// values which specify `at <position>`).
    /// https://drafts.fxtf.org/motion-1/#valdef-offset-path-basic-shape
    Context,
}

bitflags! {
    /// The flags to represent which basic shapes we would like to support.
    ///
    /// Different properties may use different subsets of <basic-shape>:
    /// e.g.
    /// clip-path: all basic shapes.
    /// motion-path: all basic shapes (but ignore fill-rule).
    /// shape-outside: inset(), circle(), ellipse(), polygon().
    ///
    /// Also there are some properties we don't support for now:
    /// shape-inside: inset(), circle(), ellipse(), polygon().
    /// SVG shape-inside and shape-subtract: circle(), ellipse(), polygon().
    ///
    /// The spec issue proposes some better ways to clarify the usage of basic shapes, so for now
    /// we use the bitflags to choose the supported basic shapes for each property at the parse
    /// time.
    /// https://github.com/w3c/csswg-drafts/issues/7390
    #[cfg_attr(feature = "gecko", derive(Clone, Copy))]
    #[repr(C)]
    pub struct AllowedBasicShapes: u8 {
        /// inset().
        const INSET = 1 << 0;
        /// xywh().
        const XYWH = 1 << 1;
        // TODO: Bug 1786161. Add rect().
        // const RECT = 1 << 2;
        /// circle().
        const CIRCLE = 1 << 3;
        /// ellipse().
        const ELLIPSE = 1 << 4;
        /// polygon().
        const POLYGON = 1 << 5;
        /// path().
        const PATH = 1 << 6;
        // TODO: Bug 1823463. Add shape().
        // const SHAPE = 1 << 7;

        /// All flags.
        const ALL =
            Self::INSET.bits |
            Self::XYWH.bits |
            Self::CIRCLE.bits |
            Self::ELLIPSE.bits |
            Self::POLYGON.bits |
            Self::PATH.bits;

        /// For shape-outside.
        const SHAPE_OUTSIDE =
            Self::INSET.bits |
            Self::CIRCLE.bits |
            Self::ELLIPSE.bits |
            Self::POLYGON.bits;
    }
}

/// A helper for both clip-path and shape-outside parsing of shapes.
fn parse_shape_or_box<'i, 't, R, ReferenceBox>(
    context: &ParserContext,
    input: &mut Parser<'i, 't>,
    to_shape: impl FnOnce(Box<BasicShape>, ReferenceBox) -> R,
    to_reference_box: impl FnOnce(ReferenceBox) -> R,
    flags: AllowedBasicShapes,
) -> Result<R, ParseError<'i>>
where
    ReferenceBox: Default + Parse,
{
    let mut shape = None;
    let mut ref_box = None;
    loop {
        if shape.is_none() {
            shape = input
                .try_parse(|i| {
                    BasicShape::parse(
                        context,
                        i,
                        flags,
                        ShapeType::Filled,
                        DefaultPosition::Center,
                    )
                })
                .ok();
        }

        if ref_box.is_none() {
            ref_box = input.try_parse(|i| ReferenceBox::parse(context, i)).ok();
            if ref_box.is_some() {
                continue;
            }
        }
        break;
    }

    if let Some(shp) = shape {
        return Ok(to_shape(Box::new(shp), ref_box.unwrap_or_default()));
    }

    match ref_box {
        Some(r) => Ok(to_reference_box(r)),
        None => Err(input.new_custom_error(StyleParseErrorKind::UnspecifiedError)),
    }
}

impl Parse for ClipPath {
    #[inline]
    fn parse<'i, 't>(
        context: &ParserContext,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self, ParseError<'i>> {
        if input.try_parse(|i| i.expect_ident_matching("none")).is_ok() {
            return Ok(ClipPath::None);
        }

        if let Ok(url) = input.try_parse(|i| SpecifiedUrl::parse(context, i)) {
            return Ok(ClipPath::Url(url));
        }

        parse_shape_or_box(
            context,
            input,
            ClipPath::Shape,
            ClipPath::Box,
            AllowedBasicShapes::ALL,
        )
    }
}

impl Parse for ShapeOutside {
    #[inline]
    fn parse<'i, 't>(
        context: &ParserContext,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self, ParseError<'i>> {
        // Need to parse this here so that `Image::parse_with_cors_anonymous`
        // doesn't parse it.
        if input.try_parse(|i| i.expect_ident_matching("none")).is_ok() {
            return Ok(ShapeOutside::None);
        }

        if let Ok(image) = input.try_parse(|i| Image::parse_with_cors_anonymous(context, i)) {
            debug_assert_ne!(image, Image::None);
            return Ok(ShapeOutside::Image(image));
        }

        parse_shape_or_box(
            context,
            input,
            ShapeOutside::Shape,
            ShapeOutside::Box,
            AllowedBasicShapes::SHAPE_OUTSIDE,
        )
    }
}

impl BasicShape {
    /// Parse with some parameters.
    /// 1. The supported <basic-shape>.
    /// 2. The type of shapes. Should we ignore fill-rule?
    /// 3. The default value of `at <position>`.
    pub fn parse<'i, 't>(
        context: &ParserContext,
        input: &mut Parser<'i, 't>,
        flags: AllowedBasicShapes,
        shape_type: ShapeType,
        default_position: DefaultPosition,
    ) -> Result<Self, ParseError<'i>> {
        let location = input.current_source_location();
        let function = input.expect_function()?.clone();
        input.parse_nested_block(move |i| {
            match_ignore_ascii_case! { &function,
                "inset" if flags.contains(AllowedBasicShapes::INSET) => {
                    InsetRect::parse_function_arguments(context, i)
                        .map(BasicShapeRect::Inset)
                        .map(BasicShape::Rect)
                },
                "circle" if flags.contains(AllowedBasicShapes::CIRCLE) => {
                    Circle::parse_function_arguments(context, i, default_position)
                        .map(BasicShape::Circle)
                },
                "ellipse" if flags.contains(AllowedBasicShapes::ELLIPSE) => {
                    Ellipse::parse_function_arguments(context, i, default_position)
                        .map(BasicShape::Ellipse)
                },
                "polygon" if flags.contains(AllowedBasicShapes::POLYGON) => {
                    Polygon::parse_function_arguments(context, i, shape_type)
                        .map(BasicShape::Polygon)
                },
                "path" if flags.contains(AllowedBasicShapes::PATH) => {
                    Path::parse_function_arguments(i, shape_type).map(BasicShape::Path)
                },
                "xywh"
                    if flags.contains(AllowedBasicShapes::XYWH)
                        && static_prefs::pref!("layout.css.basic-shape-xywh.enabled") =>
                {
                    Xywh::parse_function_arguments(context, i)
                        .map(BasicShapeRect::Xywh)
                        .map(BasicShape::Rect)
                },
                _ => Err(location
                    .new_custom_error(StyleParseErrorKind::UnexpectedFunction(function.clone()))),
            }
        })
    }
}

impl Parse for InsetRect {
    fn parse<'i, 't>(
        context: &ParserContext,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self, ParseError<'i>> {
        input.expect_function_matching("inset")?;
        input.parse_nested_block(|i| Self::parse_function_arguments(context, i))
    }
}

impl InsetRect {
    /// Parse the inner function arguments of `inset()`
    fn parse_function_arguments<'i, 't>(
        context: &ParserContext,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self, ParseError<'i>> {
        let rect = Rect::parse_with(context, input, LengthPercentage::parse)?;
        let round = if input
            .try_parse(|i| i.expect_ident_matching("round"))
            .is_ok()
        {
            BorderRadius::parse(context, input)?
        } else {
            BorderRadius::zero()
        };
        Ok(generic::InsetRect { rect, round })
    }
}

fn parse_at_position<'i, 't>(
    context: &ParserContext,
    input: &mut Parser<'i, 't>,
    default_position: DefaultPosition,
) -> Result<PositionOrAuto, ParseError<'i>> {
    if input.try_parse(|i| i.expect_ident_matching("at")).is_ok() {
        Position::parse(context, input).map(PositionOrAuto::Position)
    } else {
        // FIXME: Bug 1837340. Per spec issue, https://github.com/w3c/csswg-drafts/issues/8695, we
        // may not serialize the optional `at <position>` for all basic shapes. So we will drop
        // this later.
        match default_position {
            DefaultPosition::Center => Ok(PositionOrAuto::Position(Position::center())),
            DefaultPosition::Context => Ok(PositionOrAuto::Auto),
        }
    }
}

impl Parse for Circle {
    fn parse<'i, 't>(
        context: &ParserContext,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self, ParseError<'i>> {
        input.expect_function_matching("circle")?;
        input.parse_nested_block(|i| {
            Self::parse_function_arguments(context, i, DefaultPosition::Center)
        })
    }
}

impl Circle {
    fn parse_function_arguments<'i, 't>(
        context: &ParserContext,
        input: &mut Parser<'i, 't>,
        default_position: DefaultPosition,
    ) -> Result<Self, ParseError<'i>> {
        let radius = input
            .try_parse(|i| ShapeRadius::parse(context, i))
            .unwrap_or_default();
        let position = parse_at_position(context, input, default_position)?;

        Ok(generic::Circle { radius, position })
    }
}

impl Parse for Ellipse {
    fn parse<'i, 't>(
        context: &ParserContext,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self, ParseError<'i>> {
        input.expect_function_matching("ellipse")?;
        input.parse_nested_block(|i| {
            Self::parse_function_arguments(context, i, DefaultPosition::Center)
        })
    }
}

impl Ellipse {
    fn parse_function_arguments<'i, 't>(
        context: &ParserContext,
        input: &mut Parser<'i, 't>,
        default_position: DefaultPosition,
    ) -> Result<Self, ParseError<'i>> {
        let (semiaxis_x, semiaxis_y) = input
            .try_parse(|i| -> Result<_, ParseError> {
                Ok((
                    ShapeRadius::parse(context, i)?,
                    ShapeRadius::parse(context, i)?,
                ))
            })
            .unwrap_or_default();
        let position = parse_at_position(context, input, default_position)?;

        Ok(generic::Ellipse {
            semiaxis_x,
            semiaxis_y,
            position,
        })
    }
}

fn parse_fill_rule<'i, 't>(input: &mut Parser<'i, 't>, shape_type: ShapeType) -> FillRule {
    match shape_type {
        // Per [1] and [2], we ignore `<fill-rule>` for outline shapes, so always use a default
        // value.
        // [1] https://github.com/w3c/csswg-drafts/issues/3468
        // [2] https://github.com/w3c/csswg-drafts/issues/7390
        //
        // Also, per [3] and [4], we would like the ignore `<file-rule>` from outline shapes, e.g.
        // offset-path, which means we don't parse it when setting `ShapeType::Outline`.
        // This should be web compatible because the shipped "offset-path:path()" doesn't have
        // `<fill-rule>` and "offset-path:polygon()" is a new feature and still behind the
        // preference.
        // [3] https://github.com/w3c/fxtf-drafts/issues/512#issuecomment-1545393321
        // [4] https://github.com/w3c/fxtf-drafts/issues/512#issuecomment-1555330929
        ShapeType::Outline => Default::default(),
        ShapeType::Filled => input
            .try_parse(|i| -> Result<_, ParseError> {
                let fill = FillRule::parse(i)?;
                i.expect_comma()?;
                Ok(fill)
            })
            .unwrap_or_default(),
    }
}

impl Parse for Polygon {
    fn parse<'i, 't>(
        context: &ParserContext,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self, ParseError<'i>> {
        input.expect_function_matching("polygon")?;
        input.parse_nested_block(|i| Self::parse_function_arguments(context, i, ShapeType::Filled))
    }
}

impl Polygon {
    /// Parse the inner arguments of a `polygon` function.
    fn parse_function_arguments<'i, 't>(
        context: &ParserContext,
        input: &mut Parser<'i, 't>,
        shape_type: ShapeType,
    ) -> Result<Self, ParseError<'i>> {
        let fill = parse_fill_rule(input, shape_type);
        let coordinates = input
            .parse_comma_separated(|i| {
                Ok(PolygonCoord(
                    LengthPercentage::parse(context, i)?,
                    LengthPercentage::parse(context, i)?,
                ))
            })?
            .into();

        Ok(Polygon { fill, coordinates })
    }
}

impl Path {
    /// Parse the inner arguments of a `path` function.
    fn parse_function_arguments<'i, 't>(
        input: &mut Parser<'i, 't>,
        shape_type: ShapeType,
    ) -> Result<Self, ParseError<'i>> {
        use crate::values::specified::svg_path::AllowEmpty;

        let fill = parse_fill_rule(input, shape_type);
        let path = SVGPathData::parse(input, AllowEmpty::No)?;
        Ok(Path { fill, path })
    }
}

impl ToCss for Xywh {
    fn to_css<W>(&self, dest: &mut CssWriter<W>) -> fmt::Result
    where
        W: Write,
    {
        self.x.to_css(dest)?;
        dest.write_char(' ')?;
        self.y.to_css(dest)?;
        dest.write_char(' ')?;
        self.width.to_css(dest)?;
        dest.write_char(' ')?;
        self.height.to_css(dest)?;
        if !self.round.is_zero() {
            dest.write_str(" round ")?;
            self.round.to_css(dest)?;
        }
        Ok(())
    }
}

impl Xywh {
    /// Parse the inner function arguments of `xywh()`
    fn parse_function_arguments<'i, 't>(
        context: &ParserContext,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self, ParseError<'i>> {
        let x = LengthPercentage::parse(context, input)?;
        let y = LengthPercentage::parse(context, input)?;
        let width = NonNegativeLengthPercentage::parse(context, input)?;
        let height = NonNegativeLengthPercentage::parse(context, input)?;
        let round = if input
            .try_parse(|i| i.expect_ident_matching("round"))
            .is_ok()
        {
            BorderRadius::parse(context, input)?
        } else {
            BorderRadius::zero()
        };

        Ok(Xywh {
            x,
            y,
            width,
            height,
            round,
        })
    }
}

impl ToComputedValue for BasicShapeRect {
    type ComputedValue = ComputedInsetRect;

    #[inline]
    fn to_computed_value(&self, context: &Context) -> Self::ComputedValue {
        use crate::values::computed::LengthPercentage;
        use style_traits::values::specified::AllowedNumericType;

        match self {
            Self::Inset(ref inset) => inset.to_computed_value(context),
            Self::Xywh(ref xywh) => {
                // Given `xywh(x y w h)`, construct the equivalent inset() function,
                // `inset(y calc(100% - x - w) calc(100% - y - h) x)`.
                //
                // https://drafts.csswg.org/css-shapes-1/#basic-shape-computed-values
                // https://github.com/w3c/csswg-drafts/issues/9053
                let x = xywh.x.to_computed_value(context);
                let y = xywh.y.to_computed_value(context);
                let w = xywh.width.to_computed_value(context);
                let h = xywh.height.to_computed_value(context);
                // calc(100% - x - w).
                let right = LengthPercentage::hundred_percent_minus_list(
                    &[&x, &w.0],
                    AllowedNumericType::All,
                );
                // calc(100% - y - h).
                let bottom = LengthPercentage::hundred_percent_minus_list(
                    &[&y, &h.0],
                    AllowedNumericType::All,
                );

                ComputedInsetRect {
                    rect: Rect::new(y, right, bottom, x),
                    round: xywh.round.to_computed_value(context),
                }
            },
        }
    }

    #[inline]
    fn from_computed_value(computed: &Self::ComputedValue) -> Self {
        Self::Inset(ToComputedValue::from_computed_value(computed))
    }
}

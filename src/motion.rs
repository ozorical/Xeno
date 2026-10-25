use std::time::Duration;

use windows::{
    Foundation::{TimeSpan, TypedEventHandler},
    UI::{
        Color,
        Composition::{Desktop::DesktopWindowTarget, *},
    },
    Win32::{Foundation::HWND, System::WinRT::Composition::ICompositorDesktopInterop},
    core::*,
};
use windows_numerics::{Vector2, Vector3};

use crate::app::{Event, emit};

const CORNERS: [&str; 4] = ["TopLeftRadius", "TopRightRadius", "BottomLeftRadius", "BottomRightRadius"];

#[derive(Clone, Copy, PartialEq)]
pub struct Pose {
    pub opacity: f32,
    pub scale: f32,
    pub x: f32,
    pub radius: f32,
}

impl Pose {
    pub const REST: Self = Self { opacity: 1.0, scale: 1.0, x: 0.0, radius: 0.0 };

    pub fn new(opacity: f32, scale: f32) -> Self {
        Self { opacity, scale, ..Self::REST }
    }

    pub fn x(self, x: f32) -> Self {
        Self { x, ..self }
    }

    pub fn round(self, radius: f32) -> Self {
        Self { radius, ..self }
    }
}

#[derive(Clone, Copy)]
pub enum Curve {
    Glide,
    Pop,
    Exit,
}

pub struct Layer {
    pub visual: ContainerVisual,
    clip: RectangleClip,
}

pub struct Motion {
    comp: Compositor,
    _target: DesktopWindowTarget,
    root: ContainerVisual,
    back: CompositionColorBrush,
    tabs: ContainerVisual,
    pub shell: ContainerVisual,
    line: SpriteVisual,
    tint: CompositionColorBrush,
    glide: CubicBezierEasingFunction,
    pop: CubicBezierEasingFunction,
    exit: CubicBezierEasingFunction,
    crawl: CubicBezierEasingFunction,
    pub speed: f32,
    size: Vector2,
}

pub fn color(hex: &str) -> Color {
    let hex = hex.trim_start_matches('#');
    let part = |i: usize| hex.get(i..i + 2).and_then(|s| u8::from_str_radix(s, 16).ok()).unwrap_or(128);
    Color { A: 255, R: part(0), G: part(2), B: part(4) }
}

fn v2(x: f32, y: f32) -> Vector2 {
    Vector2 { X: x, Y: y }
}

fn v3(x: f32, y: f32, z: f32) -> Vector3 {
    Vector3 { X: x, Y: y, Z: z }
}

fn span(ms: f32) -> TimeSpan {
    TimeSpan::from(Duration::from_secs_f32(ms.max(1.0) / 1000.0))
}

impl Motion {
    pub fn new(hwnd: HWND, back: Color, accent: Color) -> Result<Self> {
        let comp = Compositor::new()?;
        let interop: ICompositorDesktopInterop = comp.cast()?;
        let target = unsafe { interop.CreateDesktopWindowTarget(hwnd, false)? };
        let full = v2(1.0, 1.0);

        let root = comp.CreateContainerVisual()?;
        root.SetRelativeSizeAdjustment(full)?;
        root.SetOpacity(0.0)?;

        let brush = comp.CreateColorBrushWithColor(back)?;
        let fill = comp.CreateSpriteVisual()?;
        fill.SetRelativeSizeAdjustment(full)?;
        fill.SetBrush(&brush)?;

        let tabs = comp.CreateContainerVisual()?;
        tabs.SetRelativeSizeAdjustment(full)?;
        let shell = comp.CreateContainerVisual()?;
        shell.SetRelativeSizeAdjustment(full)?;

        let tint = comp.CreateColorBrushWithColor(accent)?;
        let line = comp.CreateSpriteVisual()?;
        line.SetBrush(&tint)?;
        line.SetOpacity(0.0)?;

        let children = root.Children()?;
        children.InsertAtTop(&fill)?;
        children.InsertAtTop(&tabs)?;
        children.InsertAtTop(&shell)?;
        children.InsertAtTop(&line)?;
        target.SetRoot(&root)?;

        Ok(Self {
            glide: comp.CreateCubicBezierEasingFunction(v2(0.16, 1.0), v2(0.3, 1.0))?,
            pop: comp.CreateCubicBezierEasingFunction(v2(0.3, 1.22), v2(0.52, 1.0))?,
            exit: comp.CreateCubicBezierEasingFunction(v2(0.45, 0.0), v2(0.8, 0.3))?,
            crawl: comp.CreateCubicBezierEasingFunction(v2(0.1, 0.6), v2(0.3, 1.0))?,
            comp,
            _target: target,
            root,
            back: brush,
            tabs,
            shell,
            line,
            tint,
            speed: 1.0,
            size: v2(0.0, 0.0),
        })
    }

    pub fn resize(&mut self, w: f32, h: f32, dpi: f32) -> Result<()> {
        self.size = v2(w, h);
        self.root.SetCenterPoint(v3(w / 2.0, h / 2.0, 0.0))?;
        self.line.SetSize(v2(w, (2.0 * dpi).round()))?;
        Ok(())
    }

    pub fn colors(&self, back: Color, accent: Color) -> Result<()> {
        self.back.SetColor(back)?;
        self.tint.SetColor(accent)
    }

    pub fn layer(&self) -> Result<Layer> {
        let visual = self.comp.CreateContainerVisual()?;
        let clip = self.comp.CreateRectangleClip()?;
        visual.SetClip(&clip)?;
        visual.SetIsVisible(false)?;
        self.tabs.Children()?.InsertAtBottom(&visual)?;
        let layer = Layer { visual, clip };
        layer.fit(self.size)?;
        Ok(layer)
    }

    pub fn fit(&self, layer: &Layer) -> Result<()> {
        layer.fit(self.size)
    }

    pub fn raise(&self, layer: &Layer) -> Result<()> {
        let children = self.tabs.Children()?;
        children.Remove(&layer.visual)?;
        children.InsertAtTop(&layer.visual)
    }

    pub fn discard(&self, layer: &Layer) -> Result<()> {
        self.tabs.Children()?.Remove(&layer.visual)
    }

    fn curve(&self, curve: Curve) -> &CubicBezierEasingFunction {
        match curve {
            Curve::Glide => &self.glide,
            Curve::Pop => &self.pop,
            Curve::Exit => &self.exit,
        }
    }

    fn scalar(&self, to: f32, curve: Curve, ms: f32) -> Result<ScalarKeyFrameAnimation> {
        let a = self.comp.CreateScalarKeyFrameAnimation()?;
        a.InsertKeyFrameWithEasingFunction(1.0, to, self.curve(curve))?;
        a.SetDuration(span(ms * self.speed))?;
        Ok(a)
    }

    fn vector(&self, to: Vector3, curve: Curve, ms: f32) -> Result<Vector3KeyFrameAnimation> {
        let a = self.comp.CreateVector3KeyFrameAnimation()?;
        a.InsertKeyFrameWithEasingFunction(1.0, to, self.curve(curve))?;
        a.SetDuration(span(ms * self.speed))?;
        Ok(a)
    }

    fn corner(&self, to: f32, curve: Curve, ms: f32) -> Result<Vector2KeyFrameAnimation> {
        let a = self.comp.CreateVector2KeyFrameAnimation()?;
        a.InsertKeyFrameWithEasingFunction(1.0, v2(to, to), self.curve(curve))?;
        a.SetDuration(span(ms * self.speed))?;
        Ok(a)
    }

    pub fn pose(&self, layer: &Layer, p: Pose) -> Result<()> {
        let v = &layer.visual;
        for prop in ["Opacity", "Scale", "Offset"] {
            v.StopAnimation(&HSTRING::from(prop))?;
        }
        v.SetOpacity(p.opacity)?;
        v.SetScale(v3(p.scale, p.scale, 1.0))?;
        v.SetOffset(v3(p.x, 0.0, 0.0))?;
        let r = v2(p.radius, p.radius);
        for c in CORNERS {
            layer.clip.StopAnimation(&HSTRING::from(c))?;
        }
        layer.clip.SetTopLeftRadius(r)?;
        layer.clip.SetTopRightRadius(r)?;
        layer.clip.SetBottomLeftRadius(r)?;
        layer.clip.SetBottomRightRadius(r)
    }

    pub fn animate(&self, layer: &Layer, p: Pose, curve: Curve, ms: f32) -> Result<()> {
        if self.speed <= 0.0 {
            return self.pose(layer, p);
        }
        let v = &layer.visual;
        let fade = if matches!(curve, Curve::Pop) { Curve::Glide } else { curve };
        v.StartAnimation(h!("Opacity"), &self.scalar(p.opacity, fade, ms * 0.7)?)?;
        v.StartAnimation(h!("Scale"), &self.vector(v3(p.scale, p.scale, 1.0), curve, ms)?)?;
        v.StartAnimation(h!("Offset"), &self.vector(v3(p.x, 0.0, 0.0), curve, ms)?)?;
        let corner = self.corner(p.radius, Curve::Glide, ms)?;
        for c in CORNERS {
            layer.clip.StartAnimation(&HSTRING::from(c), &corner)?;
        }
        Ok(())
    }

    pub fn batch(&self, token: u64, f: impl FnOnce() -> Result<()>) -> Result<()> {
        let batch = self.comp.CreateScopedBatch(CompositionBatchTypes::Animation)?;
        let result = f();
        batch.End()?;
        batch.Completed(&TypedEventHandler::new(move |_, _| {
            emit(Event::Settled(token));
            Ok(())
        }))?;
        result
    }

    pub fn enter(&self) -> Result<()> {
        let speed = self.speed.max(0.35);
        self.root.SetScale(v3(0.965, 0.965, 1.0))?;
        let fade = self.comp.CreateScalarKeyFrameAnimation()?;
        fade.InsertKeyFrameWithEasingFunction(1.0, 1.0, &self.glide)?;
        fade.SetDuration(span(380.0 * speed))?;
        let grow = self.comp.CreateVector3KeyFrameAnimation()?;
        grow.InsertKeyFrameWithEasingFunction(1.0, v3(1.0, 1.0, 1.0), &self.glide)?;
        grow.SetDuration(span(620.0 * speed))?;
        self.root.StartAnimation(h!("Opacity"), &fade)?;
        self.root.StartAnimation(h!("Scale"), &grow)
    }

    pub fn leave(&self, token: u64) -> Result<()> {
        let speed = self.speed.max(0.35);
        self.batch(token, || {
            let fade = self.comp.CreateScalarKeyFrameAnimation()?;
            fade.InsertKeyFrameWithEasingFunction(1.0, 0.0, &self.exit)?;
            fade.SetDuration(span(190.0 * speed))?;
            let shrink = self.comp.CreateVector3KeyFrameAnimation()?;
            shrink.InsertKeyFrameWithEasingFunction(1.0, v3(0.975, 0.975, 1.0), &self.exit)?;
            shrink.SetDuration(span(210.0 * speed))?;
            self.root.StartAnimation(h!("Opacity"), &fade)?;
            self.root.StartAnimation(h!("Scale"), &shrink)
        })
    }

    pub fn loading(&self, on: bool) -> Result<()> {
        let line = &self.line;
        if on {
            line.StopAnimation(h!("Opacity"))?;
            line.StopAnimation(h!("Scale"))?;
            line.SetOpacity(1.0)?;
            line.SetScale(v3(0.0, 1.0, 1.0))?;
            let crawl = self.comp.CreateVector3KeyFrameAnimation()?;
            crawl.InsertKeyFrameWithEasingFunction(1.0, v3(0.86, 1.0, 1.0), &self.crawl)?;
            crawl.SetDuration(span(6000.0))?;
            line.StartAnimation(h!("Scale"), &crawl)
        } else {
            let finish = self.comp.CreateVector3KeyFrameAnimation()?;
            finish.InsertKeyFrameWithEasingFunction(1.0, v3(1.0, 1.0, 1.0), &self.glide)?;
            finish.SetDuration(span(220.0))?;
            let fade = self.comp.CreateScalarKeyFrameAnimation()?;
            fade.InsertKeyFrameWithEasingFunction(1.0, 0.0, &self.glide)?;
            fade.SetDelayTime(span(160.0))?;
            fade.SetDuration(span(360.0))?;
            line.StartAnimation(h!("Scale"), &finish)?;
            line.StartAnimation(h!("Opacity"), &fade)
        }
    }
}

impl Layer {
    fn fit(&self, size: Vector2) -> Result<()> {
        self.visual.SetSize(size)?;
        self.visual.SetCenterPoint(v3(size.X / 2.0, size.Y / 2.0, 0.0))?;
        self.clip.SetRight(size.X)?;
        self.clip.SetBottom(size.Y)
    }

    pub fn show(&self, on: bool) -> Result<()> {
        self.visual.SetIsVisible(on)
    }
}

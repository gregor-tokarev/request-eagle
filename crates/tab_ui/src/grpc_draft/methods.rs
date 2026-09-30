use gpui_kit::component::{
    ActiveTheme as _, Icon, h_flex,
    searchable_list::{SearchableGroup, SearchableListItem, SearchableVec},
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use request::{GrpcMethod, GrpcService, MethodKind};

/// The method picker's rows, grouped by service.
pub(super) type MethodList = SearchableVec<SearchableGroup<MethodItem>>;

#[derive(Clone)]
pub(super) struct MethodItem {
    service: SharedString,
    method: GrpcMethod,
}

pub(super) fn method_list(services: &[GrpcService]) -> MethodList {
    SearchableVec::new(
        services
            .iter()
            .map(|service| {
                let name = SharedString::from(service.name.clone());

                SearchableGroup::new(name.clone()).items(service.methods.iter().map(|method| {
                    MethodItem {
                        service: name.clone(),
                        method: method.clone(),
                    }
                }))
            })
            .collect::<Vec<_>>(),
    )
}

impl SearchableListItem for MethodItem {
    type Value = String;

    fn title(&self) -> SharedString {
        self.method.name.clone().into()
    }

    fn display_title(&self) -> Option<AnyElement> {
        Some(
            method_title(&self.service, &self.method.name, Some(self.method.kind))
                .into_any_element(),
        )
    }

    fn render(&self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        h_flex()
            .gap_2()
            .child(kind_icon(self.method.kind, cx))
            .child(self.method.name.clone())
    }

    fn value(&self) -> &Self::Value {
        &self.method.path
    }

    fn matches(&self, query: &str) -> bool {
        let query = query.to_lowercase();

        self.method.name.to_lowercase().contains(&query)
            || self.service.to_lowercase().contains(&query)
    }
}

/// `Service / Method`, with the kind icon when the definition is loaded.
pub(super) fn method_title(
    service: &str,
    method: &str,
    kind: Option<MethodKind>,
) -> impl IntoElement + use<> {
    // Show the service's own name; the package is in the picker's heading.
    let service = service.rsplit('.').next().unwrap_or(service).to_owned();
    let method = method.to_owned();

    h_flex()
        .debug_selector(|| "grpc-method-title".into())
        .min_w_0()
        .gap_2()
        .when_some(kind, |row, kind| {
            row.child(div().flex_none().child(KindIcon(kind)))
        })
        .child(
            h_flex()
                .min_w_0()
                .gap_1()
                .overflow_hidden()
                .child(div().flex_none().opacity(0.7).child(service))
                .child(div().flex_none().opacity(0.7).child("/"))
                .child(div().min_w_0().text_ellipsis().child(method)),
        )
}

/// Arrows like Postman's: a doubled arrow marks the side that streams.
pub(crate) fn kind_icon(kind: MethodKind, cx: &App) -> Icon {
    let (path, color) = match kind {
        MethodKind::Unary => ("icons/grpc-unary.svg", cx.theme().info),
        MethodKind::ServerStreaming => ("icons/grpc-server-streaming.svg", cx.theme().warning),
        MethodKind::ClientStreaming => ("icons/grpc-client-streaming.svg", cx.theme().success),
        MethodKind::BidiStreaming => ("icons/grpc-bidi-streaming.svg", cx.theme().danger),
    };

    Icon::default()
        .path(path)
        .size(rems(1.))
        .flex_none()
        .text_color(color)
}

#[derive(IntoElement)]
struct KindIcon(MethodKind);

impl RenderOnce for KindIcon {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        kind_icon(self.0, cx)
    }
}

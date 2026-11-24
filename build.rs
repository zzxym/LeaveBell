fn main() {
    #[cfg(windows)]
    {
        embed_resource::compile("assets/app.rc", embed_resource::NONE)
            .manifest_required()
            .expect("嵌入 exe 图标失败");
    }
}

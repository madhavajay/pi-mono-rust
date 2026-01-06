use pi::AgentMessage;

#[test]
fn deserializes_legacy_hook_message_role() {
    let json = r#"{
        "role": "hookMessage",
        "customType": "ext",
        "content": "hello",
        "display": true,
        "timestamp": 0
    }"#;

    let message: AgentMessage = serde_json::from_str(json).expect("deserialize hookMessage");

    match message {
        AgentMessage::HookMessage(hook) => {
            assert_eq!(hook.custom_type, "ext");
            assert!(hook.display);
        }
        other => panic!("expected HookMessage, got {other:?}"),
    }
}

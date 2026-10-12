const zh = {
  newChat: '新建聊天', search: '搜索聊天', chats: '你的聊天', local: '此设备', settings: '设置', collapse: '收起侧栏', expand: '展开侧栏',
  greeting: '有什么可以帮忙的？', placeholder: '向 CAIdex 发送消息', disclaimer: 'CAIdex 的回答可能有误。请核查重要信息。',
  model: '选择模型', configure: '配置模型', noModel: '添加模型以开始聊天', modelHint: '使用你自己的模型与 API Key。对话保存在此设备。',
  send: '发送消息', stop: '停止生成', attach: '添加附件', removeAttachment: '移除附件', retry: '重试加载', loading: '正在加载…',
  general: '常规', appearance: '外观', language: '语言', models: '模型与提供商', about: '关于 CAIdex',
  theme: '主题', system: '跟随系统', light: '浅色', dark: '深色', zh: '简体中文', en: 'English',
  addModel: '添加模型', edit: '编辑', name: '显示名称', provider: '提供商', endpoint: 'API 地址', nativeModel: '模型 ID',
  key: 'API Key', keyHelp: '密钥仅保存在 Windows 系统凭据中。已保存密钥不会回显。', keyPlaceholder: '输入新密钥；留空保留已保存密钥',
  save: '保存', cancel: '取消', close: '关闭', configured: '密钥已配置', unconfigured: '尚未配置密钥', noKey: '无需密钥', removeKey: '移除密钥',
  removeKeyConfirm: '移除此模型的 API Key？已保存的聊天会保留。', reuseKeyConfirm: 'API 地址已改变。确认将原有密钥发送到新地址？',
  saved: '配置已保存', basicOnly: '文本聊天 · 其他能力尚未验证', codex: 'Codex 将在后续阶段接入',
  localNotice: '本轮仅提供此设备的 Chat。账户、记忆与云同步尚未接入。', export: '导出聊天', copy: '复制', copied: '已复制',
  branch: '分支', branchHint: '选择此回复作为下一条消息的父节点', noResults: '没有匹配的聊天', draftHint: '草稿保存在此设备',
  generated: '正在生成', cancelled: '已停止', interrupted: '输出中断，未自动重发', failed: '生成失败，未自动重发',
  fatal: '无法打开本地数据', fatalHelp: '请确认使用安装后的 Windows 应用，且本机数据目录可用。重试不会删除已有聊天。',
  pending: '尚未完成', attachmentHint: '最多 4 个附件，每个不超过 8 MiB。模型对附件的支持须实际验证。',
  noHistory: '你的聊天会显示在这里', welcome: '从一个问题开始', privacy: '本地历史受系统目录权限保护；不是端到端加密。',
  version: '版本', licenses: '开源许可证', draftError: '草稿保存失败，请保留当前输入。', keyDestination: '此设备 · 本地 Chat',
};
const en: typeof zh = {
  newChat: 'New chat', search: 'Search chats', chats: 'Your chats', local: 'This device', settings: 'Settings', collapse: 'Close sidebar', expand: 'Open sidebar',
  greeting: 'What can I help with?', placeholder: 'Message CAIdex', disclaimer: 'CAIdex can make mistakes. Check important information.',
  model: 'Choose a model', configure: 'Configure model', noModel: 'Add a model to start chatting', modelHint: 'Use your own model and API key. Chats stay on this device.',
  send: 'Send message', stop: 'Stop generating', attach: 'Add attachment', removeAttachment: 'Remove attachment', retry: 'Reload', loading: 'Loading…',
  general: 'General', appearance: 'Appearance', language: 'Language', models: 'Models & providers', about: 'About CAIdex',
  theme: 'Theme', system: 'System', light: 'Light', dark: 'Dark', zh: '简体中文', en: 'English',
  addModel: 'Add model', edit: 'Edit', name: 'Display name', provider: 'Provider', endpoint: 'API endpoint', nativeModel: 'Model ID',
  key: 'API key', keyHelp: 'Keys stay in Windows Credential Manager. Saved keys are never displayed.', keyPlaceholder: 'Enter a new key; leave blank to keep the saved key',
  save: 'Save', cancel: 'Cancel', close: 'Close', configured: 'Key configured', unconfigured: 'Key not configured', noKey: 'No key required', removeKey: 'Remove key',
  removeKeyConfirm: 'Remove this model’s API key? Saved chats will remain.', reuseKeyConfirm: 'The API endpoint has changed. Send the existing key to the new endpoint?',
  saved: 'Configuration saved', basicOnly: 'Text chat · other capabilities unverified', codex: 'Codex will be connected in a later milestone',
  localNotice: 'This milestone provides local Chat. Account, Memory and cloud sync are not connected.', export: 'Export chat', copy: 'Copy', copied: 'Copied',
  branch: 'Branch', branchHint: 'Use this reply as the parent of the next message', noResults: 'No matching chats', draftHint: 'Drafts stay on this device',
  generated: 'Generating', cancelled: 'Stopped', interrupted: 'Output interrupted; not automatically resent', failed: 'Generation failed; not automatically resent',
  fatal: 'Unable to open local data', fatalHelp: 'Use the installed Windows app and check that its local data directory is available. Reloading will not delete chats.',
  pending: 'Incomplete', attachmentHint: 'Up to 4 files, each at most 8 MiB. Attachment support requires model verification.',
  noHistory: 'Your chats will appear here', welcome: 'Start with a question', privacy: 'Local history uses OS directory permissions; it is not end-to-end encrypted.',
  version: 'Version', licenses: 'Open-source licenses', draftError: 'Could not save the draft. Keep your current input.', keyDestination: 'This device · local Chat',
};
export const translations = { zh, en };
export type Text = typeof zh;
const errors: Record<string, [string, string]> = {
  invalid_or_sensitive_input: ['输入过长、无效或含有敏感凭据，请检查。', 'Input is too large, invalid or contains credentials.'],
  credential_unavailable: ['系统凭据存储不可用，请检查 Windows 权限。', 'Windows credential storage is unavailable.'],
  key_missing: ['请先在设置中添加此模型的 API Key。', 'Add an API key for this model in Settings.'],
  model_unavailable: ['请先配置并选择模型。', 'Configure and select a model first.'],
  turn_active: ['请先停止当前生成，再更改模型配置。', 'Stop the current generation before changing the model.'],
  invalid_configuration: ['模型配置无效，请检查模型 ID 和地址。', 'Check the model ID and endpoint.'],
  invalid_endpoint: ['仅允许 HTTPS 或本机回环 HTTP 地址，不允许内嵌密钥。', 'Use HTTPS or literal loopback HTTP, without embedded credentials.'],
  key_destination_changed: ['地址已改变，请确认密钥目标或输入新的密钥。', 'Confirm the changed key destination or enter a new key.'],
  storage_unavailable: ['本地存储不可用，已有内容不会自动删除。', 'Local storage is unavailable. Existing data will not be deleted.'],
  cancelled: ['生成已停止。', 'Generation stopped.'],
  already_submitted: ['这条消息已经提交，未重复发送。', 'This message was already submitted and was not resent.'],
  new_profile_required: ['更换提供商时请添加新模型配置。', 'Add a new profile to change providers.'],
};
export function errorText(error: unknown, language: 'zh' | 'en') {
  return (errors[String(error)] ?? ['操作未完成，请检查配置或本机存储后重试。', 'The operation failed. Check configuration or local storage.'])[language === 'zh' ? 0 : 1];
}

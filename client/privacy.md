# Privacy Policy

> Version: 1.0
> Date: 2026-07-08
> Applicable Software: AI-SubTrans v1.0.4

## Version Information

This privacy policy applies to AI-SubTrans v1.0.4 current version.

### Current Version
- No user account system
- No personal data collection
- Local data storage only
- No telemetry or analytics

### Future Versions
Future versions may include the following features. When these features are added, this privacy policy will be updated and users will be notified:
- User account system (username, email)
- Points/credits system
- Paid API services
- Cloud synchronization

---

## Data Collection

AI-SubTrans only collects the following data locally:

1. **Translation History**: Stored in local SQLite database, including:
   - File name/path
   - Translation timestamp
   - Translation API used
   - Original text and translated text (for history review and editing)

2. **Application Settings**: Stored locally, including:
   - API Keys (encrypted storage)
   - UI settings
   - Player settings

**We do NOT collect**:
- Personal information (name, email, IP address, etc.)
- Usage statistics
- Device fingerprint
- Any form of telemetry data

## Data Transmission

AI-SubTrans only transmits data in the following situations:

1. **Translation Requests**: When users use the translation feature, subtitle text is sent to the user-configured translation API.
   - Content: Subtitle text (original)
   - Destination: API endpoint configured by user in settings (can be third-party APIs like Baidu, Bing, Google, DeepSeek, or user's own API)
   - Method: HTTPS encryption
   - **User Control**: Users can switch to other translation APIs at any time, or use their own API Keys

2. **Auto Update Check**: Automatically checks for updates 5 seconds after startup.
   - Content: Version number
   - Destination: `https://terry2010.github.io/polly-subtitle-translator/latest.json`
   - Method: HTTPS encryption
   - **User Control**: Users can disable auto update check in settings

**Important**:
- Data transmission destination for translation is entirely determined by user configuration
- Users can choose not to use translation feature and not transmit any subtitle text
- Users can view and modify configured API endpoints at any time
- The application does not transmit data to any server without user knowledge

## Data Storage

All data is stored on the user's local device:

- **Windows**: `%APPDATA%\ai-subtrans\`
- **macOS**: `~/Library/Application Support/ai-subtrans/`

**NOT uploaded to any server**:
- Translation history is stored locally only
- Application settings are stored locally only
- API Keys are encrypted and stored locally

## User Control

Users have the following control:

1. **Clear History**: Settings page provides "Clear History" button
2. **Switch Translation API**: Users can switch to other translation APIs at any time
3. **Disable Auto Update**: Auto update check can be disabled in settings
4. **Delete Application Data**: Users can choose to delete all local data when uninstalling

## Third-Party Services

AI-SubTrans uses the following third-party services. Their privacy policies are the responsibility of their respective providers:

- **Translation API**: Translation API configured by user in settings (such as Baidu, Bing, Google, DeepSeek, or user's own API)
- **Update Check**: GitHub Pages (version information only)

**Note**:
- The privacy policy of the translation API selected by the user is the responsibility of that service provider
- If users use their own API, they are responsible for the data processing and privacy protection of that API

## Data Security

- API Keys are encrypted using system keyring
- Local database uses SQLite without additional encryption
- All network transmissions use HTTPS

## Contact

For privacy-related questions, please contact:

- GitHub Issues: https://github.com/terry2010/polly-subtitle-translator/issues

## Policy Updates

If this privacy policy is updated, a new version will be published in the GitHub repository, and users will be notified when the application is updated.

---

# 隐私政策

> 版本：1.0
> 日期：2026-07-08
> 适用软件：AI-SubTrans v1.0.4

## 版本说明

本隐私政策适用于 AI-SubTrans v1.0.4 当前版本。

### 当前版本
- 无用户账户系统
- 不收集个人身份信息
- 仅本地数据存储
- 无遥测或使用统计

### 未来版本
未来版本可能添加以下功能。届时将更新隐私政策并通知用户：
- 用户账户系统（用户名、邮箱）
- 积分系统
- 付费 API 服务
- 云端同步功能

---

## 数据收集

AI-SubTrans 仅在本地收集以下数据：

1. **翻译历史记录**：存储在本地 SQLite 数据库中，包括：
   - 文件名/路径
   - 翻译时间
   - 使用的翻译 API
   - 原文和译文（用于历史回溯和编辑）

2. **应用配置**：存储在本地，包括：
   - API Key（加密存储）
   - 界面设置
   - 播放器设置

**不收集**：
- 用户个人信息（姓名、邮箱、IP 地址等）
- 使用行为统计
- 设备指纹
- 任何形式的遥测数据

## 数据传输

AI-SubTrans 仅在以下情况下传输数据：

1. **翻译请求**：当用户使用翻译功能时，字幕文本会发送到用户配置的翻译 API。
   - 传输内容：字幕文本（原文）
   - 传输目标：用户在设置中配置的 API 端点（可以是第三方 API 如百度、Bing、Google、DeepSeek，也可以是用户自己搭建的 API）
   - 传输方式：HTTPS 加密
   - **用户控制**：用户可以随时切换到其他翻译 API，或使用自己的 API Key

2. **自动更新检查**：启动后 5 秒自动检查更新。
   - 传输内容：版本号
   - 传输目标：`https://terry2010.github.io/polly-subtitle-translator/latest.json`
   - 传输方式：HTTPS 加密
   - **用户控制**：用户可以在设置中禁用自动更新检查

**重要**：
- 翻译时的数据传输目标完全由用户配置决定
- 用户可以选择不使用翻译功能，不传输任何字幕文本
- 用户可以随时查看和修改已配置的 API 端点
- 应用不会在用户不知情的情况下传输数据到任何服务器

## 数据存储

所有数据均存储在用户本地设备：

- **Windows**：`%APPDATA%\ai-subtrans\`
- **macOS**：`~/Library/Application Support/ai-subtrans/`

**不上传到任何服务器**：
- 翻译历史记录仅存储在本地
- 应用配置仅存储在本地
- API Key 加密存储在本地

## 用户控制

用户拥有以下控制权：

1. **清除历史记录**：设置页提供"清除历史记录"按钮
2. **切换翻译 API**：用户可随时切换到其他翻译 API
3. **禁用自动更新**：设置页可关闭自动更新检查
4. **删除应用数据**：卸载应用时可选择删除所有本地数据

## 第三方服务

AI-SubTrans 使用以下第三方服务，其隐私政策由各自服务提供商负责：

- **翻译 API**：用户在设置中配置的翻译 API（如百度、Bing、Google、DeepSeek，或用户自建的 API）
- **更新检查**：GitHub Pages（仅获取版本信息）

**注意**：
- 用户选择使用哪个翻译 API，该 API 的隐私政策由该服务提供商负责
- 如果用户使用自己搭建的 API，则由用户自行负责该 API 的数据处理和隐私保护

## 数据安全

- API Key 使用系统 keyring 加密存储
- 本地数据库使用 SQLite，无额外加密
- 所有网络传输使用 HTTPS

## 联系方式

如有隐私相关问题，请通过以下方式联系：

- GitHub Issues：https://github.com/terry2010/polly-subtitle-translator/issues

## 政策更新

本隐私政策如有更新，将在 GitHub 仓库中发布新版本，并在应用更新时提示用户。
# discord-channel-archiver

A small discord bot to archive the messages in a discord text channel.

## Usage

- Edit `src/main.rs` and change the value of the constant `OUTPUT_DIRECTORY` to your desired download location.
- [Create](https://discordpy.readthedocs.io/en/latest/discord.html#creating-a-bot-account) a discord application and bot.
- [Invite](https://discordpy.readthedocs.io/en/latest/discord.html#inviting-your-bot) the bot to your server.
- Run the bot with `cargo run`. To provide the token, you have 3 options:
  - Provide the token directly with `--token <token>`
  - Provide the name of a file containing the token with `--token-filename <filename>`
  - Set the environment variable `DISCORD_TOKEN` to the token before running.
- Send the command `!archive <channel> [mode(s)]`, where `<channel>` is the channel you want to archive, and `[mode(s)]` is a possibly comma-separated list of modes. Valid modes are: `json,html`. All modes are enabled if this parameter is omitted.
- Sit back and watch the bot export the channel to the file format(s) you requested.

## Incomplete features

I have some planned features that I am yet to finish (or even start) implementing:

- HTML output, similar to discord's own interface .
  - Dark mode and Light mode support
- YAML output.

---

Based on [Serenity](https://github.com/serenity-rs/serenity).

Inspired by [this](https://github.com/Tyrrrz/DiscordChatExporter) similar program.

The HTML / CSS template from [DiscordChatExporter](https://github.com/Tyrrrz/DiscordChatExporter) is used, under the terms of the GNU GPL.

Available under the terms of the GNU AGPL.

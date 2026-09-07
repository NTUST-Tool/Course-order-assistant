# Notification topic migration

Notifications now use a randomly generated 256-bit topic, stored in `course_assistant_config.json` beside the executable. Student IDs and hardware identifiers are no longer required.

## Upgrade

1. Start monitoring. A legacy configuration without a random topic is migrated and saved.
2. Subscribe to the newly displayed topic in the ntfy app. The previous topic will no longer receive this application's notifications.
3. Keep the configuration file to retain the same topic across restarts. Moving the executable without its configuration creates a different topic.
4. If configuration loading or saving fails, resolve the reported error before monitoring can start. Do not delete a damaged configuration without first making a private backup.

## Privacy limits

The topic consists of 64 random hexadecimal characters, matching ntfy's 64-character maximum. Earlier unreleased 71-character topics are repaired by removing the `course-` prefix; re-subscribe if that format was previously saved. Keep it and the configuration private. Unix configuration files are restricted to the owner; on Windows, access follows the containing directory's permissions.

A random topic is difficult to guess, but it is not encryption or authentication. Anyone who obtains the topic may be able to subscribe or publish. The ntfy service receives notification content; do not include sensitive information. Older MD5/SHA-256 and hardware-binding security claims are obsolete.

## Delivery and verification

Only successful vacancy notifications count toward the 12-notification limit and five-minute spacing. Failed deliveries are retried on a subsequent polling cycle. Q followed by Enter cancels monitoring even during a request or retry; EOF exits cleanly.

Local tests inspect real outgoing notification headers and text using a loopback server and cover failed delivery accounting. They never send notifications to a public topic. Live phone delivery requires an approved destination and is not established by those tests.

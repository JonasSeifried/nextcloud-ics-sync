# Nextcloud ICS Sync

A simple, efficient command-line tool to synchronize a remote iCalendar (`.ics`) feed to an existing Nextcloud calendar using WebDAV and CalDAV. It keeps your Nextcloud calendar up-to-date with an external source, handling additions, updates, and deletions automatically while ignoring events not imported with this tool.

This is perfect for syncing read-only calendars from services like university schedules, public holidays, or other systems that provide an ICS link.

## Features

- **One-Way Sync**: Synchronizes events from a source ICS URL to a target Nextcloud calendar.
- **Efficient Updates**: Only uploads new or modified events (based on the `LAST-MODIFIED` timestamp) and deletes events that are no longer in the source feed.
- **Parallel Operations**: Uploads and deletions are performed concurrently for faster synchronization, especially with large calendars.
- **Authentication Support**: Supports basic authentication for source ICS feeds that require a username and password.
- **Calendar Discovery**: Includes a utility to list all available calendar IDs for your Nextcloud user, simplifying setup.
- **Configurable Logging**: Uses `env_logger` for detailed operational insight.

## Configuration

The application is configured entirely through environment variables. You can place these in a `.env` file in the same directory as the executable.

| Variable             | Required | Description                                                                              |
| -------------------- | :------: | ---------------------------------------------------------------------------------------- |
| `NEXTCLOUD_URL`      |   Yes    | The base URL of your Nextcloud instance (e.g., `https://cloud.example.com`).             |
| `NEXTCLOUD_USERNAME` |   Yes    | Your Nextcloud username.                                                                 |
| `NEXTCLOUD_PASSWORD` |   Yes    | Your Nextcloud app password or user password. **An app password is highly recommended.** |
| `CALENDAR_ID`        |   Yes    | The ID of the target calendar in Nextcloud. Use `FETCH_CALENDARS=true` to find this.     |
| `ICS_URL`            |   Yes    | The full URL of the source `.ics` calendar feed.                                         |
| `ICS_USERNAME`       |    No    | The username for basic authentication on the source ICS feed, if required.               |
| `ICS_PASSWORD`       |    No    | The password for basic authentication on the source ICS feed, if required.               |
| `RUST_LOG`           |    No    | Sets the logging level. E.g., `INFO`, `DEBUG`, `WARN`, `ERROR` (default).                |

### Example `.env` file

```

NEXTCLOUD_URL=https://nextcloud.example.com
NEXTCLOUD_USERNAME=myuser
NEXTCLOUD_PASSWORD=xxxx-xxxx-xxxx-xxxx

# Required for sync
CALENDAR_ID=work
ICS_URL=https://example.com/path/to/my/work/calendar.ics

# Optionals
RUST_LOG=INFO
ICS_USERNAME=user
ICS_PASSWORD=xxxx-xxxx-xxxx-xxxx
```

## Usage

### 1. Find your Calendar ID

If you don't know the ID of the calendar you want to sync to, you can find it easily:

1.  Create a temporary `.env` file with your `NEXTCLOUD_*` credentials.
1.  Run the application: `./nextcloud-ics-sync fetch`
1.  The application will print a list of your available calendars and their corresponding IDs, like `Available Calendars: [personal,work,birthdays]`.
1.  Copy the desired ID into your final `.env` file as `CALENDAR_ID`.

### 2. Run the Sync

Once your `.env` file is fully configured, simply run the executable:

```sh
./nextcloud-ics-sync
```

The application will perform the sync and log its progress to the console. You can run this executable on a schedule (e.g., using a cron job or a systemd timer) to keep your calendar continuously updated.

### 3. Automation

You can run this executable on a schedule (e.g., using a **cron job** or a **systemd timer**) to keep your calendar continuously updated.

## Building from Source

1.  Ensure you have the Rust toolchain installed.
2.  Clone this repository.
3.  Build the release executable:
    ```sh
    cargo build --release
    ```
4.  The binary will be located at `target/release/nextcloud-ics-sync`.

## Deletion / Clean-Up

To delete all synced events execute:

```sh
./nextcloud-ics-sync delete
```

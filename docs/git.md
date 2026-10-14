# Git

Collections are plain files, so Git can keep their history and share them
with your team. Request Eagle commits, pulls, pushes and syncs them for you,
and shows where each repository stands.

## Track collections with Git

Collections live in `~/.request-eagle/collections` unless you open them from
somewhere else. To track them with Git, click the branch icon in the status
bar at the bottom of the window and choose **Track with Git**. To share them,
create an empty repository on GitHub or another host, paste its address in
the Git panel and click **Publish**.

## Keep a collection in your project

Click the folder icon in the Collections section, or run **Open folder as
collection** from the command palette, and choose a folder in your project's
repository, such as an empty `api` folder. The collection stays where it is
and shows in the sidebar with a link icon. Requests you save there are files
in your project, reviewed and versioned with your code.

The folder must be empty or be a collection itself, with a request, its
variables or its settings at the top. Request Eagle remembers the folders you
open on this device; **Remove collection** in a linked collection's menu
forgets one and leaves its files as they are. A folder that is gone, as on an
unplugged drive, shows again once it is back.

## Commit, pull, push and sync

The status bar shows the branch, the commits to pull (↓) and to push (↑) and
how many changes are not committed. A collection with uncommitted changes
shows their count beside its name. Click the status bar's Git item, or press
Ctrl+Shift+G (Command+Shift+G on macOS), to open the Git panel. For each
repository it lists the changes and offers:

- **Commit**: commits the collections' changes, with the message you type or
  one naming the collections that changed. Press Enter in the message to
  commit.
- **Pull**: takes the remote's new commits. Your own commits are replayed on
  top of them unless your Git settings choose otherwise.
- **Push**: sends your commits. A new branch starts tracking the remote.
- **Sync**: commits, pulls and pushes in one go.

The command palette has **Sync with Git**, **Pull changes** and **Push
commits**, which act on every repository with a remote, and **Commit
changes**, which opens the Git panel to type a message.

Request Eagle commits only the collections' files. In a project's repository,
your code and anything you staged stay out of the commit. Pulling puts your
uncommitted work aside and back; what you had staged comes back unstaged.

After a pull, or when you switch branches in a terminal, the sidebar and
open tabs show the new requests. Tabs with unsaved changes keep them, and
saving asks whose changes to keep.

## Conflicts

When your changes and the remote's touch the same request, the pull stops,
or leaves the conflict in the request when your changes were not committed.
The Git panel and the status bar show the conflict, and the sidebar counts
the request it cannot read. Resolve the conflict in your editor or terminal,
finish the rebase or merge with Git, and sync again.

## Things to know

- Request Eagle runs the `git` you have installed, with your settings, keys
  and credential helpers. It never asks for a password: sign in to your Git
  host the way you do in a terminal first.
- Request Eagle checks the remotes for new commits every few minutes, and
  reads the repositories again whenever it comes back to the front.
- A collection's variables are committed with it. Keep tokens and passwords
  in environments, which stay on your device.

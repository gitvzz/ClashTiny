ObjC.import('Cocoa');
ObjC.import('Foundation');

const app = $.NSApplication.sharedApplication;
app.setActivationPolicy($.NSApplicationActivationPolicyAccessory);

// Cmd+C / Cmd+V / Cmd+X / Cmd+A
const menubar = $.NSMenu.alloc.init;
const editMenuItem = $.NSMenuItem.alloc.initWithTitleActionKeyEquivalent($("Edit"), null, $(""));
const editMenu = $.NSMenu.alloc.initWithTitle($("Edit"));
editMenu.addItemWithTitleActionKeyEquivalent($("Cut"), "cut:", $("x"));
editMenu.addItemWithTitleActionKeyEquivalent($("Copy"), "copy:", $("c"));
editMenu.addItemWithTitleActionKeyEquivalent($("Paste"), "paste:", $("v"));
editMenu.addItemWithTitleActionKeyEquivalent($("Select All"), "selectAll:", $("a"));
editMenuItem.submenu = editMenu;
menubar.addItem(editMenuItem);
app.mainMenu = menubar;

const width = 380;
const urlBoxHeight = 50;
const container = $.NSView.alloc.initWithFrame($.NSMakeRect(0, 0, width, 168));

const errorLabel = $.NSTextField.labelWithString($(""));
errorLabel.frame = $.NSMakeRect(0, 148, width, 16);
errorLabel.font = $.NSFont.systemFontOfSize(11);
errorLabel.textColor = $.NSColor.systemRedColor;
container.addSubview(errorLabel);

const urlLabel = $.NSTextField.labelWithString($("订阅链接："));
urlLabel.frame = $.NSMakeRect(0, 126, width, 18);
urlLabel.font = $.NSFont.systemFontOfSize(12);
container.addSubview(urlLabel);

// Multi-line scrollable text view for long URLs
const urlScrollView = $.NSScrollView.alloc.initWithFrame($.NSMakeRect(0, 74, width, urlBoxHeight));
urlScrollView.hasVerticalScroller = true;
urlScrollView.borderType = $.NSBezelBorder;
const urlTextView = $.NSTextView.alloc.initWithFrame($.NSMakeRect(0, 0, width - 4, urlBoxHeight - 4));
urlTextView.font = $.NSFont.systemFontOfSize(13);
urlTextView.richText = false;
urlTextView.allowsUndo = true;
urlTextView.autoresizingMask = $.NSViewWidthSizable;
urlTextView.textContainer.widthTracksTextView = true;
urlScrollView.documentView = urlTextView;
container.addSubview(urlScrollView);

const nameLabel = $.NSTextField.labelWithString($("名称："));
nameLabel.frame = $.NSMakeRect(0, 50, width, 18);
nameLabel.font = $.NSFont.systemFontOfSize(12);
container.addSubview(nameLabel);

const nameField = $.NSTextField.alloc.initWithFrame($.NSMakeRect(0, 26, width, 22));
nameField.placeholderString = $("给订阅起个名字");
nameField.font = $.NSFont.systemFontOfSize(13);
container.addSubview(nameField);

const overwriteCheck = $.NSButton.alloc.initWithFrame($.NSMakeRect(0, 0, width, 20));
overwriteCheck.setButtonType($.NSSwitchButton);
overwriteCheck.title = $("覆盖同名订阅");
overwriteCheck.state = $.NSControlStateValueOn;
overwriteCheck.font = $.NSFont.systemFontOfSize(12);
container.addSubview(overwriteCheck);

const alert = $.NSAlert.alloc.init;
alert.messageText = $("添加订阅");
alert.informativeText = $("");
alert.addButtonWithTitle($("确定"));
alert.addButtonWithTitle($("取消"));
alert.accessoryView = container;
alert.window.initialFirstResponder = urlTextView;
alert.window.level = $.NSFloatingWindowLevel;

const fm = $.NSFileManager.defaultManager;
const profilesDir = $("~/.config/clash-tiny/profiles").stringByExpandingTildeInPath.js;

while (true) {
    app.activateIgnoringOtherApps(true);
    const response = alert.runModal;

    if (response !== $.NSAlertFirstButtonReturn) {
        break;
    }

    const url = urlTextView.string.js.replace(/[\r\n]/g, "").trim();
    const name = nameField.stringValue.js.trim();

    if (url.length === 0 || name.length === 0) {
        errorLabel.stringValue = $("请填写订阅链接和名称");
        continue;
    }

    if (!url.startsWith("http://") && !url.startsWith("https://")) {
        errorLabel.stringValue = $("订阅链接必须以 http:// 或 https:// 开头");
        continue;
    }

    const overwrite = overwriteCheck.state === $.NSControlStateValueOn;
    if (!overwrite) {
        const profilePath = $(profilesDir + "/" + name + ".yaml");
        if (fm.fileExistsAtPath(profilePath)) {
            errorLabel.stringValue = $("订阅「" + name + "」已存在，请勾选覆盖或使用其他名称");
            continue;
        }
    }

    const ow = overwrite ? "1" : "0";
    url + "\x1F" + name + "\x1F" + ow;
    break;
}

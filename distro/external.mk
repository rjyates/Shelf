# Weston repaint hook sources. The headless patch compiles these after they land in the tree.
define OSEINK_WESTON_DAMAGE
	cp $(BR2_EXTERNAL_OSEINK_PATH)/panel/oseink-refresh.c $(@D)/libweston/backend-headless/oseink-refresh.c
	cp $(BR2_EXTERNAL_OSEINK_PATH)/panel/oseink-refresh.h $(@D)/libweston/backend-headless/oseink-refresh.h
endef
WESTON_POST_PATCH_HOOKS += OSEINK_WESTON_DAMAGE

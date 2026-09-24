/* pam_mura_test.so — TEST-ONLY PAM module for the mura-authd conformance harness
 * (specs/session-auth.md §6 items 2 and 7). Never in a shipped image.
 *
 *   auth  ... pam_mura_test.so sleep=5      — sleeps N seconds, then succeeds (item 2)
 *   auth  ... pam_mura_test.so batched      — one conversation callback carrying two prompts
 *                                              + one info; succeeds if the first response is
 *                                              "batched-ok" (item 7)
 */
#include <security/pam_modules.h>
#include <security/pam_appl.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

int pam_sm_authenticate(pam_handle_t *pamh, int flags, int argc, const char **argv)
{
	(void)flags;
	int batched = 0;
	for (int i = 0; i < argc; i++) {
		if (strncmp(argv[i], "sleep=", 6) == 0)
			sleep((unsigned)atoi(argv[i] + 6));
		else if (strcmp(argv[i], "batched") == 0)
			batched = 1;
	}
	if (!batched)
		return PAM_SUCCESS;

	const struct pam_conv *conv;
	if (pam_get_item(pamh, PAM_CONV, (const void **)&conv) != PAM_SUCCESS || !conv)
		return PAM_CONV_ERR;
	struct pam_message m[3] = {
		{ PAM_PROMPT_ECHO_OFF, "Secret:" },
		{ PAM_PROMPT_ECHO_ON, "Visible:" },
		{ PAM_TEXT_INFO, "Info line" },
	};
	const struct pam_message *mp[3] = { &m[0], &m[1], &m[2] };
	struct pam_response *r = NULL;
	int rc = conv->conv(3, mp, &r, conv->appdata_ptr);
	if (rc != PAM_SUCCESS || !r)
		return PAM_CONV_ERR;
	int ok = r[0].resp && strcmp(r[0].resp, "batched-ok") == 0 && r[1].resp && r[2].resp == NULL;
	for (int i = 0; i < 3; i++)
		free(r[i].resp);
	free(r);
	return ok ? PAM_SUCCESS : PAM_AUTH_ERR;
}

int pam_sm_setcred(pam_handle_t *pamh, int flags, int argc, const char **argv)
{
	(void)pamh; (void)flags; (void)argc; (void)argv;
	return PAM_SUCCESS;
}

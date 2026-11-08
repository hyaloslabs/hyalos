// A starter policy for a first job (`hyalos policy starter`). It lets the fix-issue workflow read
// the repository and its issue, push the job's own branch and open a pull request, with the job's
// data going only to the model and the forge. Read it before you approve it: it is your decision.
//
// The shipped baseline's forbids still apply under it: no write outside the job's repository, no
// push outside the job's branch, and no irreversible action, such as a merge, without your approval.
// It permits no merge and no web host; add those when you want them (docs/operations/running-a-job.md).

permit (
  principal,
  action in [Action::"read", Action::"push", Action::"open_pr", Action::"data_flow"],
  resource
);

// A job that read forge content never sends data to the web, whatever hosts it asks for.
forbid (principal, action == Action::"data_flow", resource)
  when { principal.tainted && resource.name == "job->web" };

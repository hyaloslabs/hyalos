// A starter policy for a first issue (`hyalos policy starter`). It lets the fix-issue workflow read
// the repository and its issue, push the job's own branch and open a pull request, with the job's
// data going only to the model and the forge. It also lets you merge that pull request with
// `hyalos merge` once you have reviewed it. Read it before you approve it: it is your decision.
//
// The shipped baseline's forbids still apply under it: no write outside the job's repository, no
// push outside the job's branch, no merge by a job that read forge content, and no irreversible
// action, such as a merge, without your signed approval of that pull request's head. So no job
// merges anything: each merge is yours, one `hyalos merge` at a time (ADR-0071, ADR-0082).
// It permits no web host; add one when you want it (docs/operations/running-a-job.md).

permit (
  principal,
  action in [Action::"read", Action::"push", Action::"open_pr", Action::"data_flow", Action::"merge"],
  resource
);

// A job that read forge content never sends data to the web, whatever hosts it asks for.
forbid (principal, action == Action::"data_flow", resource)
  when { principal.tainted && resource.name == "job->web" };

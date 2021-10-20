pub type Var = String;

pub enum Term {
    Var (Var),
    Lam (Var, Term),
    App (Term, Term),
    Prod (Term, Term),
    Prim (String),
}

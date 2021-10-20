use indexmap::map::IndexMap;

enum Type {
    Com,
    Exp,
    Var,
    Cross (Type, Type),
    Arrow (Type, Type),
}

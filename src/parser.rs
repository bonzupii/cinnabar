use crate::ast::BinOp;
use crate::ast::EnumVariant;
use crate::ast::Expr;
use crate::ast::GenericParam;
use crate::ast::Item;
use crate::ast::ItemKind;
use crate::ast::Lit;
use crate::ast::Param;
use crate::ast::Pattern;
use crate::ast::Spanned;
use crate::ast::Stmt;
use crate::ast::StructField;
use crate::ast::Type;
use crate::ast::TypeKind;
use crate::ast::UnOp;
use crate::lexer::Span;
use crate::lexer::Token;
use crate::lexer::TokenKind;
use std::fmt;

#[derive(Debug, Clone)]
pub struct ParseError {
    pub message: String,
    pub span: Span,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Parse Error at line {}, col {}: {}",
            self.span.line, self.span.col, self.message
        )
    }
}

enum DeclKind {
    Unknown,
    Struct,
    Enum,
}

pub struct Parser<'a> {
    tokens: &'a [Token],
    cursor: usize,
}

impl<'a> Parser<'a> {
    pub fn new(tokens: &'a [Token]) -> Self {
        Self { tokens, cursor: 0 }
    }

    pub fn parse_program(&mut self) -> Result<Vec<Spanned<Item>>, ParseError> {
        let mut items = Vec::new();
        while !self.is_at_end() && self.peek_kind() != Some(&TokenKind::Eof) {
            let doc_comment = self.collect_doc_comments();
            if self.is_at_end() || self.peek_kind() == Some(&TokenKind::Eof) {
                break;
            }
            let start_span = self.current_span();
            let item_node = self.parse_item(doc_comment)?;
            let end_span = self.previous_span();
            let full_span = Span::new(start_span.start, end_span.end, start_span.line, start_span.col);
            items.push(Spanned::new(item_node, full_span));
        }
        Ok(items)
    }

    fn collect_doc_comments(&mut self) -> Option<String> {
        let mut docs = Vec::new();
        while let Some(TokenKind::DocComment(text)) = self.peek_kind() {
            docs.push(text.clone());
            self.advance();
        }
        if docs.is_empty() {
            None
        } else {
            Some(docs.join("\n"))
        }
    }

    fn peek_can_start_type(&self) -> bool {
        matches!(
            self.peek_kind(),
            Some(TokenKind::PascalIdent(_)) | Some(TokenKind::Ampersand) | Some(TokenKind::LBracket)
        )
    }

    fn parse_item(&mut self, doc: Option<String>) -> Result<Item, ParseError> {
        let start_span = self.current_span();

        let is_pub = if self.match_kind(&TokenKind::Pub) {
            self.advance();
            true
        } else {
            false
        };

        let is_native = if self.match_kind(&TokenKind::Native) {
            self.advance();
            true
        } else {
            false
        };

        let kind = match self.peek_kind() {
            Some(TokenKind::Const) => {
                if is_native {
                    return Err(ParseError {
                        message: "The 'native' modifier cannot be used on 'const' declarations".to_string(),
                        span: start_span,
                    });
                }
                self.parse_const(is_pub)?
            }
            Some(TokenKind::Type) => self.parse_type_decl(is_pub, is_native)?,
            Some(TokenKind::Mod) => {
                if is_native {
                    return Err(ParseError {
                        message: "The 'native' modifier cannot be used on 'mod' declarations".to_string(),
                        span: start_span,
                    });
                }
                self.parse_module(is_pub)?
            }
            Some(TokenKind::Use) => {
                if is_native {
                    return Err(ParseError {
                        message: "The 'native' modifier cannot be used on 'use' imports".to_string(),
                        span: start_span,
                    });
                }
                self.parse_use(is_pub)?
            }
            Some(TokenKind::Fun) | Some(TokenKind::Impure) => self.parse_function(is_pub, is_native)?,
            Some(TokenKind::Trait) => {
                if is_native {
                    return Err(ParseError {
                        message: "The 'native' modifier cannot be used on 'trait' declarations".to_string(),
                        span: start_span,
                    });
                }
                self.parse_trait(is_pub)?
            }
            Some(TokenKind::Impl) => {
                if is_native {
                    return Err(ParseError {
                        message: "The 'native' modifier cannot be used on 'impl' blocks".to_string(),
                        span: start_span,
                    });
                }
                self.parse_impl(is_pub)?
            }
            Some(unexpected_token) => return Err(ParseError {
                message: format!("Expected item declaration, found {:?}", unexpected_token),
                span: start_span,
            }),
            None => return Err(ParseError {
                message: "Unexpected end of input while parsing item".to_string(),
                span: start_span,
            }),
        };

        Ok(Item { doc, kind })
    }

    fn parse_const(&mut self, is_pub: bool) -> Result<ItemKind, ParseError> {
        self.expect(&TokenKind::Const)?;
        let name = self.expect_screaming_ident()?;
        self.expect(&TokenKind::Colon)?;
        let ty = self.parse_spanned_type()?;
        self.expect(&TokenKind::Eq)?;
        let init = self.parse_spanned_expr()?;
        Ok(ItemKind::Const { is_pub, name, ty, init })
    }

    fn parse_type_decl(&mut self, is_pub: bool, is_native: bool) -> Result<ItemKind, ParseError> {
        self.expect(&TokenKind::Type)?;
        let name = self.expect_pascal_ident()?;

        let generics = if self.match_kind(&TokenKind::LParen) {
            self.advance();
            let mut args = Vec::new();
            while !self.match_kind(&TokenKind::RParen) && !self.is_at_end() {
                args.push(GenericParam {
                    name: self.expect_pascal_ident()?,
                    bounds: Vec::new(),
                });
                if self.match_kind(&TokenKind::Comma) {
                    self.advance();
                } else {
                    break;
                }
            }
            self.expect(&TokenKind::RParen)?;
            args
        } else {
            Vec::new()
        };

        if is_native {
            return Ok(ItemKind::TypeDecl {
                is_pub,
                name,
                generics,
                kind: TypeKind::Native,
            });
        }

        if self.match_kind(&TokenKind::End) {
            self.advance();
            return Ok(ItemKind::TypeDecl {
                is_pub,
                name,
                generics,
                kind: TypeKind::Unit,
            });
        }

        let mut fields = Vec::new();
        let mut variants = Vec::new();
        let mut decl_kind = DeclKind::Unknown;

        while !self.match_kind(&TokenKind::End) && !self.is_at_end() {
            self.collect_doc_comments();
            if self.match_kind(&TokenKind::End) || self.is_at_end() {
                break;
            }

            let start_item_span = self.current_span();
            let field_or_variant_pub = if self.match_kind(&TokenKind::Pub) {
                self.advance();
                true
            } else {
                false
            };

            if let Ok(field_name) = self.expect_snake_ident() {
                match decl_kind {
                    DeclKind::Enum => {
                        return Err(ParseError {
                            message: "Cannot mix enum variants and struct fields in type declaration".to_string(),
                            span: start_item_span,
                        });
                    }
                    DeclKind::Unknown => decl_kind = DeclKind::Struct,
                    DeclKind::Struct => {}
                }

                self.expect(&TokenKind::Colon)?;
                let field_ty = self.parse_spanned_type()?;
                fields.push(StructField {
                    is_pub: field_or_variant_pub,
                    name: field_name,
                    ty: field_ty,
                });
            } else if let Ok(variant_name) = self.expect_pascal_ident() {
                match decl_kind {
                    DeclKind::Struct => {
                        return Err(ParseError {
                            message: "Cannot mix struct fields and enum variants in type declaration".to_string(),
                            span: start_item_span,
                        });
                    }
                    DeclKind::Unknown => decl_kind = DeclKind::Enum,
                    DeclKind::Enum => {}
                }

                let mut tuple_types = Vec::new();
                if self.match_kind(&TokenKind::LParen) {
                    self.advance();
                    while !self.match_kind(&TokenKind::RParen) && !self.is_at_end() {
                        tuple_types.push(self.parse_spanned_type()?);
                        if self.match_kind(&TokenKind::Comma) {
                            self.advance();
                        } else {
                            break;
                        }
                    }
                    self.expect(&TokenKind::RParen)?;
                }
                variants.push(EnumVariant {
                    is_pub: field_or_variant_pub,
                    name: variant_name,
                    types: tuple_types,
                });
            } else {
                return Err(ParseError {
                    message: "Expected struct field or enum variant in type definition".to_string(),
                    span: self.current_span(),
                });
            }
        }

        self.expect(&TokenKind::End)?;

        let kind = match decl_kind {
            DeclKind::Struct => TypeKind::Struct(fields),
            DeclKind::Enum => TypeKind::Enum(variants),
            DeclKind::Unknown => TypeKind::Unit,
        };

        Ok(ItemKind::TypeDecl {
            is_pub,
            name,
            generics,
            kind,
        })
    }

    fn parse_trait(&mut self, is_pub: bool) -> Result<ItemKind, ParseError> {
        self.expect(&TokenKind::Trait)?;
        let name = self.expect_pascal_ident()?;
        let mut methods = Vec::new();

        while !self.match_kind(&TokenKind::End) && !self.is_at_end() {
            let doc_comment = self.collect_doc_comments();
            if self.match_kind(&TokenKind::End) || self.is_at_end() {
                break;
            }

            let start_span = self.current_span();
            let method_pub = if self.match_kind(&TokenKind::Pub) {
                self.advance();
                true
            } else {
                false
            };

            self.expect(&TokenKind::Fun)?;
            let method_name = self.expect_snake_ident()?;

            self.expect(&TokenKind::LParen)?;
            let params = self.parse_params()?;
            self.expect(&TokenKind::RParen)?;

            let is_impure = if self.match_kind(&TokenKind::Impure) {
                self.advance();
                true
            } else {
                false
            };

            let return_ty = if self.peek_can_start_type() {
                Some(self.parse_spanned_type()?)
            } else {
                None
            };

            let end_span = self.previous_span();
            let method_span = Span::new(start_span.start, end_span.end, start_span.line, start_span.col);

            let method_item = Item {
                doc: doc_comment,
                kind: ItemKind::Function {
                    is_pub: method_pub,
                    is_native: false,
                    is_impure,
                    name: method_name,
                    generics: Vec::new(),
                    params,
                    return_ty,
                    body: None,
                },
            };
            methods.push(Spanned::new(method_item, method_span));
        }
        self.expect(&TokenKind::End)?;

        Ok(ItemKind::Trait { is_pub, name, methods })
    }

    fn parse_impl(&mut self, is_pub: bool) -> Result<ItemKind, ParseError> {
        self.expect(&TokenKind::Impl)?;
        let trait_name = self.expect_pascal_ident()?;
        self.expect(&TokenKind::For)?;
        let target_type = self.parse_spanned_type()?;
        let mut methods = Vec::new();

        while !self.match_kind(&TokenKind::End) && !self.is_at_end() {
            let doc_comment = self.collect_doc_comments();
            if self.match_kind(&TokenKind::End) || self.is_at_end() {
                break;
            }

            let start_span = self.current_span();
            let method_pub = if self.match_kind(&TokenKind::Pub) {
                self.advance();
                true
            } else {
                false
            };

            let method_kind = self.parse_function(method_pub, false)?;
            let end_span = self.previous_span();
            let method_span = Span::new(start_span.start, end_span.end, start_span.line, start_span.col);

            methods.push(Spanned::new(Item { doc: doc_comment, kind: method_kind }, method_span));
        }
        self.expect(&TokenKind::End)?;

        Ok(ItemKind::Impl {
            is_pub,
            trait_name,
            target_type,
            methods,
        })
    }

    fn parse_module(&mut self, is_pub: bool) -> Result<ItemKind, ParseError> {
        self.expect(&TokenKind::Mod)?;
        let name = self.expect_pascal_ident()?;

        let mut items = Vec::new();
        while !self.match_kind(&TokenKind::End) && !self.is_at_end() {
            let doc_comment = self.collect_doc_comments();
            if self.match_kind(&TokenKind::End) || self.is_at_end() {
                break;
            }
            let start_span = self.current_span();
            let item_node = self.parse_item(doc_comment)?;
            let end_span = self.previous_span();
            let full_span = Span::new(start_span.start, end_span.end, start_span.line, start_span.col);
            items.push(Spanned::new(item_node, full_span));
        }
        self.expect(&TokenKind::End)?;

        Ok(ItemKind::Module { is_pub, name, items })
    }

    fn parse_use(&mut self, is_pub: bool) -> Result<ItemKind, ParseError> {
        self.expect(&TokenKind::Use)?;
        let mut path = Vec::new();
        path.push(self.expect_pascal_ident()?);

        while self.match_kind(&TokenKind::Dot) {
            self.advance();
            if let Ok(pascal_name) = self.expect_pascal_ident() {
                path.push(pascal_name);
            } else {
                path.push(self.expect_snake_ident()?);
            }
        }

        let alias = if self.match_kind(&TokenKind::As) {
            self.advance();
            Some(self.expect_snake_ident()?)
        } else {
            None
        };

        Ok(ItemKind::Use { is_pub, path, alias })
    }

    fn parse_function(&mut self, is_pub: bool, is_native: bool) -> Result<ItemKind, ParseError> {
        self.expect(&TokenKind::Fun)?;
        let name = self.expect_snake_ident()?;

        let generics = if self.match_kind(&TokenKind::Lt) {
            self.advance();
            let mut args = Vec::new();
            while !self.match_kind(&TokenKind::Gt) && !self.is_at_end() {
                let gen_name = self.expect_pascal_ident()?;
                let mut bounds = Vec::new();
                if self.match_kind(&TokenKind::Colon) {
                    self.advance();
                    bounds.push(self.expect_pascal_ident()?);
                }
                args.push(GenericParam { name: gen_name, bounds });
                if self.match_kind(&TokenKind::Comma) {
                    self.advance();
                } else {
                    break;
                }
            }
            self.expect(&TokenKind::Gt)?;
            args
        } else {
            Vec::new()
        };

        self.expect(&TokenKind::LParen)?;
        let params = self.parse_params()?;
        self.expect(&TokenKind::RParen)?;

        let is_impure = if self.match_kind(&TokenKind::Impure) {
            self.advance();
            true
        } else {
            false
        };

        let return_ty = if self.peek_can_start_type() {
            Some(self.parse_spanned_type()?)
        } else {
            None
        };

        let body = if is_native {
            None
        } else {
            let mut stmts = Vec::new();
            while !self.match_kind(&TokenKind::End) && !self.is_at_end() {
                self.collect_doc_comments();
                if self.match_kind(&TokenKind::End) || self.is_at_end() {
                    break;
                }
                stmts.push(self.parse_spanned_stmt()?);
            }
            self.expect(&TokenKind::End)?;
            Some(stmts)
        };

        Ok(ItemKind::Function {
            is_pub,
            is_native,
            is_impure,
            name,
            generics,
            params,
            return_ty,
            body,
        })
    }

    fn parse_params(&mut self) -> Result<Vec<Param>, ParseError> {
        let mut params = Vec::new();
        while !self.match_kind(&TokenKind::RParen) && !self.is_at_end() {
            let name = self.expect_snake_ident()?;
            self.expect(&TokenKind::Colon)?;
            let ty = self.parse_spanned_type()?;
            params.push(Param { name, ty });

            if self.match_kind(&TokenKind::Comma) {
                self.advance();
            } else {
                break;
            }
        }
        Ok(params)
    }

    fn parse_spanned_type(&mut self) -> Result<Spanned<Type>, ParseError> {
        let start_span = self.current_span();
        let type_node = self.parse_type()?;
        let end_span = self.previous_span();
        let full_span = Span::new(start_span.start, end_span.end, start_span.line, start_span.col);
        Ok(Spanned::new(type_node, full_span))
    }

    fn parse_type(&mut self) -> Result<Type, ParseError> {
        if self.match_kind(&TokenKind::Ampersand) {
            self.advance();
            if self.match_kind(&TokenKind::Mut) {
                self.advance();
                let inner_type = self.parse_spanned_type()?;
                return Ok(Type::RefMut(Box::new(inner_type)));
            } else if self.match_kind(&TokenKind::LBracket) {
                self.advance();
                let inner_type = self.parse_spanned_type()?;
                self.expect(&TokenKind::RBracket)?;
                let end_span = self.previous_span();
                let inner_span = inner_type.span;
                let slice_span = Span::new(inner_span.start, end_span.end, inner_span.line, inner_span.col);
                let slice_type = Spanned::new(Type::Slice(Box::new(inner_type)), slice_span);
                return Ok(Type::Ref(Box::new(slice_type)));
            } else {
                let inner_type = self.parse_spanned_type()?;
                return Ok(Type::Ref(Box::new(inner_type)));
            }
        }

        if self.match_kind(&TokenKind::LBracket) {
            self.advance();
            let element_type = self.parse_spanned_type()?;
            if self.match_kind(&TokenKind::Semicolon) {
                self.advance();
                let size_val = match self.peek_kind() {
                    Some(TokenKind::IntLit(size)) => {
                        let sz = *size as usize;
                        self.advance();
                        sz
                    }
                    unexpected_kind => return Err(ParseError {
                        message: format!("Expected array size integer, found {:?}", unexpected_kind),
                        span: self.current_span(),
                    }),
                };
                self.expect(&TokenKind::RBracket)?;
                return Ok(Type::Array(Box::new(element_type), size_val));
            } else {
                self.expect(&TokenKind::RBracket)?;
                return Ok(Type::Slice(Box::new(element_type)));
            }
        }

        let mut path = Vec::new();
        path.push(self.expect_pascal_ident()?);

        while self.match_kind(&TokenKind::Dot) {
            self.advance();
            path.push(self.expect_pascal_ident()?);
        }

        let is_generic = self.match_kind(&TokenKind::LParen);
        let type_args = if is_generic {
            self.advance();
            let mut args = Vec::new();
            while !self.match_kind(&TokenKind::RParen) && !self.is_at_end() {
                args.push(self.parse_spanned_type()?);
                if self.match_kind(&TokenKind::Comma) {
                    self.advance();
                } else {
                    break;
                }
            }
            self.expect(&TokenKind::RParen)?;
            Some(args)
        } else {
            None
        };

        if let Some(args) = type_args {
            if path.len() == 1 {
                Ok(Type::Generic(path[0].clone(), args))
            } else {
                Ok(Type::GenericPath(path, args))
            }
        } else {
            if path.len() == 1 {
                Ok(Type::Named(path[0].clone()))
            } else {
                Ok(Type::Path(path))
            }
        }
    }

    fn parse_spanned_stmt(&mut self) -> Result<Spanned<Stmt>, ParseError> {
        let start_span = self.current_span();
        let stmt_node = self.parse_stmt()?;
        let end_span = self.previous_span();
        let full_span = Span::new(start_span.start, end_span.end, start_span.line, start_span.col);
        Ok(Spanned::new(stmt_node, full_span))
    }

    fn parse_stmt(&mut self) -> Result<Stmt, ParseError> {
        let start_span = self.current_span();

        if self.match_kind(&TokenKind::Pub) {
            self.advance();
            if self.match_kind(&TokenKind::Val) || self.match_kind(&TokenKind::Var) {
                return Err(ParseError {
                    message: "pub cannot appear on local val/var".to_string(),
                    span: start_span,
                });
            } else {
                return Err(ParseError {
                    message: "Unexpected 'pub' modifier inside local scope".to_string(),
                    span: start_span,
                });
            }
        }
        if self.match_kind(&TokenKind::Val) {
            self.advance();
            let name = self.expect_snake_ident()?;
            let ty = if self.match_kind(&TokenKind::Colon) {
                self.advance();
                Some(self.parse_spanned_type()?)
            } else {
                None
            };
            self.expect(&TokenKind::Eq)?;
            let init = self.parse_spanned_expr()?;
            Ok(Stmt::Val { name, ty, init })
        } else if self.match_kind(&TokenKind::Var) {
            self.advance();
            let name = self.expect_snake_ident()?;
            let ty = if self.match_kind(&TokenKind::Colon) {
                self.advance();
                Some(self.parse_spanned_type()?)
            } else {
                None
            };
            self.expect(&TokenKind::Eq)?;
            let init = self.parse_spanned_expr()?;
            Ok(Stmt::Var { name, ty, init })
        } else if self.match_kind(&TokenKind::While) {
            self.advance();
            let cond = self.parse_spanned_expr()?;
            let mut body = Vec::new();
            while !self.match_kind(&TokenKind::End) && !self.is_at_end() {
                self.collect_doc_comments();
                if self.match_kind(&TokenKind::End) || self.is_at_end() {
                    break;
                }
                body.push(self.parse_spanned_stmt()?);
            }
            self.expect(&TokenKind::End)?;
            Ok(Stmt::While { cond, body })
        } else if let Some(TokenKind::SnakeIdent(name)) = self.peek_kind() {
            let var_name = name.clone();
            if self.peek_next_kind() == Some(&TokenKind::Eq) {
                self.advance();
                self.advance();
                let expr = self.parse_spanned_expr()?;
                Ok(Stmt::Assign { name: var_name, expr })
            } else {
                let expr = self.parse_spanned_expr()?;
                Ok(Stmt::Expr(expr))
            }
        } else if !self.is_at_end() {
            let expr = self.parse_spanned_expr()?;
            Ok(Stmt::Expr(expr))
        } else {
            Err(ParseError {
                message: "Unexpected EOF while parsing statement".to_string(),
                span: self.current_span(),
            })
        }
    }

    fn parse_spanned_expr(&mut self) -> Result<Spanned<Expr>, ParseError> {
        let start_span = self.current_span();
        let expr_node = self.parse_expr()?;
        let end_span = self.previous_span();
        let full_span = Span::new(start_span.start, end_span.end, start_span.line, start_span.col);
        Ok(Spanned::new(expr_node, full_span))
    }

    fn parse_expr(&mut self) -> Result<Expr, ParseError> {
        if self.match_kind(&TokenKind::Return) {
            self.advance();
            let return_val = if !self.match_kind(&TokenKind::End)
                && !self.match_kind(&TokenKind::Else)
                && !self.is_at_end()
            {
                Some(Box::new(self.parse_spanned_expr()?))
            } else {
                None
            };
            return Ok(Expr::Return(return_val));
        }

        if self.match_kind(&TokenKind::Break) {
            self.advance();
            return Ok(Expr::Break);
        }

        if self.match_kind(&TokenKind::Continue) {
            self.advance();
            return Ok(Expr::Continue);
        }

        if self.match_kind(&TokenKind::If) {
            self.advance();
            let cond = self.parse_spanned_expr()?;
            let mut then_body = Vec::new();
            let mut else_body = None;

            while !self.match_kind(&TokenKind::Else) && !self.match_kind(&TokenKind::End) && !self.is_at_end() {
                self.collect_doc_comments();
                if self.match_kind(&TokenKind::Else) || self.match_kind(&TokenKind::End) || self.is_at_end() {
                    break;
                }
                then_body.push(self.parse_spanned_stmt()?);
            }

            if self.match_kind(&TokenKind::Else) {
                self.advance();
                let mut else_stmts = Vec::new();
                while !self.match_kind(&TokenKind::End) && !self.is_at_end() {
                    self.collect_doc_comments();
                    if self.match_kind(&TokenKind::End) || self.is_at_end() {
                        break;
                    }
                    else_stmts.push(self.parse_spanned_stmt()?);
                }
                else_body = Some(else_stmts);
            }
            self.expect(&TokenKind::End)?;

            return Ok(Expr::If(Box::new(cond), then_body, else_body));
        }

        if self.match_kind(&TokenKind::Match) {
            self.advance();
            let target_expr = self.parse_spanned_expr()?;
            let mut arms = Vec::new();

            while !self.match_kind(&TokenKind::End) && !self.is_at_end() {
                self.collect_doc_comments();
                if self.match_kind(&TokenKind::End) || self.is_at_end() {
                    break;
                }
                let pattern = self.parse_spanned_pattern()?;
                self.expect(&TokenKind::FatArrow)?;
                let body_expr = self.parse_spanned_expr()?;
                arms.push((pattern, body_expr));
            }
            self.expect(&TokenKind::End)?;

            return Ok(Expr::Match(Box::new(target_expr), arms));
        }

        self.parse_binary_expr(0)
    }

    fn parse_spanned_pattern(&mut self) -> Result<Spanned<Pattern>, ParseError> {
        let start_span = self.current_span();
        let pattern_node = self.parse_pattern()?;
        let end_span = self.previous_span();
        let full_span = Span::new(start_span.start, end_span.end, start_span.line, start_span.col);
        Ok(Spanned::new(pattern_node, full_span))
    }

    fn parse_pattern(&mut self) -> Result<Pattern, ParseError> {
        if self.match_kind(&TokenKind::LBracket) {
            self.advance();
            let mut patterns = Vec::new();
            while !self.match_kind(&TokenKind::RBracket) && !self.is_at_end() {
                if let Ok(name) = self.expect_snake_ident() {
                    let name_span = self.previous_span();
                    if self.match_kind(&TokenKind::At) {
                        self.advance();
                        self.expect(&TokenKind::DotDot)?;
                        patterns.push(Spanned::new(Pattern::Rest(name), name_span));
                    } else {
                        patterns.push(Spanned::new(Pattern::Var(name), name_span));
                    }
                } else {
                    patterns.push(self.parse_spanned_pattern()?);
                }

                if self.match_kind(&TokenKind::Comma) {
                    self.advance();
                } else {
                    break;
                }
            }
            self.expect(&TokenKind::RBracket)?;
            return Ok(Pattern::Array(patterns));
        }

        if let Ok(pascal_name) = self.expect_pascal_ident() {
            if self.match_kind(&TokenKind::Dot) {
                self.advance();
                let variant_name = self.expect_pascal_ident()?;
                let mut sub_patterns = Vec::new();
                if self.match_kind(&TokenKind::LParen) {
                    self.advance();
                    while !self.match_kind(&TokenKind::RParen) && !self.is_at_end() {
                        sub_patterns.push(self.parse_spanned_pattern()?);
                        if self.match_kind(&TokenKind::Comma) {
                            self.advance();
                        } else {
                            break;
                        }
                    }
                    self.expect(&TokenKind::RParen)?;
                }
                return Ok(Pattern::PathVariant(vec![pascal_name, variant_name], sub_patterns));
            } else if self.match_kind(&TokenKind::LParen) {
                self.advance();
                let mut sub_patterns = Vec::new();
                while !self.match_kind(&TokenKind::RParen) && !self.is_at_end() {
                    sub_patterns.push(self.parse_spanned_pattern()?);
                    if self.match_kind(&TokenKind::Comma) {
                        self.advance();
                    } else {
                        break;
                    }
                }
                self.expect(&TokenKind::RParen)?;
                return Ok(Pattern::Variant(pascal_name, sub_patterns));
            } else {
                return Ok(Pattern::Variant(pascal_name, Vec::new()));
            }
        }

        if let Ok(snake_name) = self.expect_snake_ident() {
            return Ok(Pattern::Var(snake_name));
        }

        match self.peek_kind() {
            Some(TokenKind::IntLit(val)) => {
                let num = *val;
                self.advance();
                Ok(Pattern::Lit(Lit::Int(num)))
            }
            Some(TokenKind::HexLit(val)) => {
                let num = *val;
                self.advance();
                Ok(Pattern::Lit(Lit::Hex(num)))
            }
            Some(TokenKind::BoolLit(val)) => {
                let b = *val;
                self.advance();
                Ok(Pattern::Lit(Lit::Bool(b)))
            }
            Some(unexpected_kind) => Err(ParseError {
                message: format!("Expected pattern, found {:?}", unexpected_kind),
                span: self.current_span(),
            }),
            None => Err(ParseError {
                message: "Unexpected EOF while parsing pattern".to_string(),
                span: self.current_span(),
            }),
        }
    }

    fn parse_binary_expr(&mut self, min_precedence: u8) -> Result<Expr, ParseError> {
        let mut left = self.parse_spanned_unary_expr()?;

        while let Some(op) = self.peek_binary_op() {
            let precedence = self.op_precedence(&op);
            if precedence < min_precedence {
                break;
            }
            self.advance();
            let right = self.parse_spanned_binary_expr(precedence + 1)?;
            let full_span = Span::new(left.span.start, right.span.end, left.span.line, left.span.col);
            left = Spanned::new(Expr::Binary(Box::new(left), op, Box::new(right)), full_span);
        }

        Ok(left.node)
    }

    fn parse_spanned_binary_expr(&mut self, min_precedence: u8) -> Result<Spanned<Expr>, ParseError> {
        let start_span = self.current_span();
        let expr_node = self.parse_binary_expr(min_precedence)?;
        let end_span = self.previous_span();
        let full_span = Span::new(start_span.start, end_span.end, start_span.line, start_span.col);
        Ok(Spanned::new(expr_node, full_span))
    }

    fn parse_spanned_unary_expr(&mut self) -> Result<Spanned<Expr>, ParseError> {
        let start_span = self.current_span();
        let expr_node = self.parse_unary_expr()?;
        let end_span = self.previous_span();
        let full_span = Span::new(start_span.start, end_span.end, start_span.line, start_span.col);
        Ok(Spanned::new(expr_node, full_span))
    }

    fn parse_unary_expr(&mut self) -> Result<Expr, ParseError> {
        if self.match_kind(&TokenKind::Not) {
            self.advance();
            let inner_expr = self.parse_spanned_unary_expr()?;
            return Ok(Expr::Unary(UnOp::Not, Box::new(inner_expr)));
        } else if self.match_kind(&TokenKind::Minus) {
            self.advance();
            let inner_expr = self.parse_spanned_unary_expr()?;
            return Ok(Expr::Unary(UnOp::Neg, Box::new(inner_expr)));
        } else if self.match_kind(&TokenKind::Ampersand) {
            self.advance();
            if self.match_kind(&TokenKind::Mut) {
                self.advance();
                let inner_expr = self.parse_spanned_unary_expr()?;
                return Ok(Expr::Unary(UnOp::RefMut, Box::new(inner_expr)));
            } else {
                let inner_expr = self.parse_spanned_unary_expr()?;
                return Ok(Expr::Unary(UnOp::Ref, Box::new(inner_expr)));
            }
        }

        self.parse_primary_expr()
    }

    fn parse_primary_expr(&mut self) -> Result<Expr, ParseError> {
        if self.match_kind(&TokenKind::Try) {
            self.advance();
            let inner_expr = self.parse_spanned_primary_expr()?;
            return Ok(Expr::Try(Box::new(inner_expr)));
        }

        let token = match self.peek_token() {
            Some(token_val) => token_val.clone(),
            None => return Err(ParseError {
                message: "Unexpected EOF while parsing expression".to_string(),
                span: self.current_span(),
            }),
        };

        let start_span = token.span;

        let mut primary_expr = match &token.kind {
            TokenKind::IntLit(val) => {
                self.advance();
                Spanned::new(Expr::Lit(Lit::Int(*val)), token.span)
            }
            TokenKind::HexLit(val) => {
                self.advance();
                Spanned::new(Expr::Lit(Lit::Hex(*val)), token.span)
            }
            TokenKind::BoolLit(val) => {
                self.advance();
                Spanned::new(Expr::Lit(Lit::Bool(*val)), token.span)
            }
            TokenKind::SnakeIdent(name) => {
                let name_str = name.clone();
                self.advance();
                Spanned::new(Expr::Var(name_str), token.span)
            }
            TokenKind::ScreamingIdent(name) => {
                let name_str = name.clone();
                self.advance();
                Spanned::new(Expr::Const(name_str), token.span)
            }
            TokenKind::PascalIdent(name) => {
                let mut path = vec![name.clone()];
                self.advance();

                while self.match_kind(&TokenKind::Dot) {
                    self.advance();
                    let segment = match self.expect_pascal_ident() {
                        Ok(pascal_seg) => pascal_seg,
                        Err(pascal_err) => {
                            match self.expect_snake_ident() {
                                Ok(snake_seg) => snake_seg,
                                Err(snake_err) => {
                                    return Err(ParseError {
                                        message: format!("Expected path segment, got {}", snake_err.message),
                                        span: pascal_err.span,
                                    });
                                }
                            }
                        }
                    };
                    path.push(segment);
                }

                let end_span = self.previous_span();
                let full_span = Span::new(token.span.start, end_span.end, token.span.line, token.span.col);

                Spanned::new(Expr::Path(path), full_span)
            }
            TokenKind::LBracket => {
                self.advance();
                let mut elements = Vec::new();
                while !self.match_kind(&TokenKind::RBracket) && !self.is_at_end() {
                    elements.push(self.parse_spanned_expr()?);
                    if self.match_kind(&TokenKind::Comma) {
                        self.advance();
                    } else {
                        break;
                    }
                }
                self.expect(&TokenKind::RBracket)?;
                let end_span = self.previous_span();
                let full_span = Span::new(token.span.start, end_span.end, token.span.line, token.span.col);
                Spanned::new(Expr::ArrayLit(elements), full_span)
            }
            TokenKind::LParen => {
                self.advance();
                let inner = self.parse_expr()?;
                self.expect(&TokenKind::RParen)?;
                let end_span = self.previous_span();
                let full_span = Span::new(token.span.start, end_span.end, token.span.line, token.span.col);
                Spanned::new(inner, full_span)
            }
            unexpected_kind => return Err(ParseError {
                message: format!("Unexpected expression token {:?}", unexpected_kind),
                span: token.span,
            }),
        };

        // Postfix Expression Loop: Field Access, Calls, Generic Instantiation, Struct Init
        loop {
            if self.match_kind(&TokenKind::Dot) {
                self.advance();
                let field_name = self.expect_snake_ident()?;
                let end_span = self.previous_span();
                let full_span = Span::new(start_span.start, end_span.end, start_span.line, start_span.col);
                primary_expr = Spanned::new(Expr::FieldAccess(Box::new(primary_expr), field_name), full_span);
            } else if self.match_kind(&TokenKind::LBracket) {
                if self.is_generic_call_ahead() {
                    self.advance();
                    let mut type_args = Vec::new();
                    while !self.match_kind(&TokenKind::RBracket) && !self.is_at_end() {
                        type_args.push(self.parse_spanned_type()?);
                        if self.match_kind(&TokenKind::Comma) {
                            self.advance();
                        } else {
                            break;
                        }
                    }
                    self.expect(&TokenKind::RBracket)?;

                    self.expect(&TokenKind::LParen)?;
                    let mut args = Vec::new();
                    while !self.match_kind(&TokenKind::RParen) && !self.is_at_end() {
                        args.push(self.parse_spanned_expr()?);
                        if self.match_kind(&TokenKind::Comma) {
                            self.advance();
                        } else {
                            break;
                        }
                    }
                    self.expect(&TokenKind::RParen)?;
                    let end_span = self.previous_span();
                    let full_span = Span::new(start_span.start, end_span.end, start_span.line, start_span.col);

                    primary_expr = Spanned::new(Expr::Call(Box::new(primary_expr), Some(type_args), args), full_span);
                } else {
                    break;
                }
            } else if self.match_kind(&TokenKind::LParen) {
                self.advance();

                if self.peek_next_kind() == Some(&TokenKind::Colon) {
                    let mut fields = Vec::new();

                    while !self.match_kind(&TokenKind::RParen) && !self.is_at_end() {
                        let field_name = self.expect_snake_ident()?;
                        self.expect(&TokenKind::Colon)?;
                        let field_val = self.parse_spanned_expr()?;
                        fields.push((field_name, field_val));

                        if self.match_kind(&TokenKind::Comma) {
                            self.advance();
                        } else {
                            break;
                        }
                    }

                    self.expect(&TokenKind::RParen)?;

                    let end_span = self.previous_span();
                    let full_span = Span::new(start_span.start, end_span.end, start_span.line, start_span.col);

                    if let Expr::Path(path) = primary_expr.node {
                        if path.len() != 1 {
                            return Err(ParseError {
                                message: "Struct initialization requires an unqualified type name".to_string(),
                                span: full_span,
                            });
                        }

                        let struct_name = match path.into_iter().next() {
                            Some(name_val) => name_val,
                            None => return Err(ParseError {
                                message: "Path expected to contain at least one segment".to_string(),
                                span: full_span,
                            }),
                        };
                        primary_expr = Spanned::new(Expr::StructInit(struct_name, fields), full_span);
                    } else {
                        return Err(ParseError {
                            message: "Struct initialization requires a type name".to_string(),
                            span: full_span,
                        });
                    }
                } else {
                    let mut args = Vec::new();

                    while !self.match_kind(&TokenKind::RParen) && !self.is_at_end() {
                        args.push(self.parse_spanned_expr()?);

                        if self.match_kind(&TokenKind::Comma) {
                            self.advance();
                        } else {
                            break;
                        }
                    }

                    self.expect(&TokenKind::RParen)?;

                    let end_span = self.previous_span();
                    let full_span = Span::new(start_span.start, end_span.end, start_span.line, start_span.col);

                    primary_expr = Spanned::new(Expr::Call(Box::new(primary_expr), None, args), full_span);
                }
            } else {
                break;
            }
        }

        Ok(primary_expr.node)
    }

    fn parse_spanned_primary_expr(&mut self) -> Result<Spanned<Expr>, ParseError> {
        let start_span = self.current_span();
        let expr_node = self.parse_primary_expr()?;
        let end_span = self.previous_span();
        let full_span = Span::new(start_span.start, end_span.end, start_span.line, start_span.col);
        Ok(Spanned::new(expr_node, full_span))
    }

    fn is_generic_call_ahead(&self) -> bool {
        let mut idx = self.cursor + 1;
        let mut bracket_depth = 1;

        while idx < self.tokens.len() {
            let token_kind = &self.tokens[idx].kind;
            if *token_kind == TokenKind::LBracket {
                bracket_depth += 1;
            } else if *token_kind == TokenKind::RBracket {
                bracket_depth -= 1;
                if bracket_depth == 0 {
                    if idx + 1 < self.tokens.len() {
                        return self.tokens[idx + 1].kind == TokenKind::LParen;
                    } else {
                        return false;
                    }
                }
            }
            idx += 1;
        }
        false
    }

    fn peek_binary_op(&self) -> Option<BinOp> {
        let kind = self.peek_kind()?;
        if *kind == TokenKind::Plus { Some(BinOp::Add) }
        else if *kind == TokenKind::Minus { Some(BinOp::Sub) }
        else if *kind == TokenKind::Star { Some(BinOp::Mul) }
        else if *kind == TokenKind::Slash { Some(BinOp::Div) }
        else if *kind == TokenKind::EqEq { Some(BinOp::Eq) }
        else if *kind == TokenKind::NotEq { Some(BinOp::NotEq) }
        else if *kind == TokenKind::Lt { Some(BinOp::Lt) }
        else if *kind == TokenKind::Gt { Some(BinOp::Gt) }
        else if *kind == TokenKind::LtEq { Some(BinOp::LtEq) }
        else if *kind == TokenKind::GtEq { Some(BinOp::GtEq) }
        else if *kind == TokenKind::Ampersand { Some(BinOp::BitAnd) }
        else if *kind == TokenKind::Pipe { Some(BinOp::BitOr) }
        else if *kind == TokenKind::Caret { Some(BinOp::BitXor) }
        else if *kind == TokenKind::Shl { Some(BinOp::Shl) }
        else if *kind == TokenKind::Shr { Some(BinOp::Shr) }
        else if *kind == TokenKind::AmpAmp { Some(BinOp::And) }
        else if *kind == TokenKind::PipePipe { Some(BinOp::Or) }
        else { None }
    }

    fn op_precedence(&self, op: &BinOp) -> u8 {
        match op {
            BinOp::Mul | BinOp::Div | BinOp::Shl | BinOp::Shr => 5,
            BinOp::Add | BinOp::Sub | BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor => 4,
            BinOp::Eq | BinOp::NotEq | BinOp::Lt | BinOp::Gt | BinOp::LtEq | BinOp::GtEq => 3,
            BinOp::And => 2,
            BinOp::Or => 1,
        }
    }

    fn expect(&mut self, expected_kind: &TokenKind) -> Result<(), ParseError> {
        if self.match_kind(expected_kind) {
            self.advance();
            Ok(())
        } else {
            Err(ParseError {
                message: format!("Expected token {:?}, found {:?}", expected_kind, self.peek_kind()),
                span: self.current_span(),
            })
        }
    }

    fn expect_snake_ident(&mut self) -> Result<String, ParseError> {
        match self.peek_kind() {
            Some(TokenKind::SnakeIdent(name)) => {
                let name_clone = name.clone();
                self.advance();
                Ok(name_clone)
            }
            Some(unexpected_kind) => Err(ParseError {
                message: format!("Expected snake_case identifier, found {:?}", unexpected_kind),
                span: self.current_span(),
            }),
            None => Err(ParseError {
                message: "Unexpected EOF while reading snake_case identifier".to_string(),
                span: self.current_span(),
            }),
        }
    }

    fn expect_pascal_ident(&mut self) -> Result<String, ParseError> {
        match self.peek_kind() {
            Some(TokenKind::PascalIdent(name)) => {
                let name_clone = name.clone();
                self.advance();
                Ok(name_clone)
            }
            Some(unexpected_kind) => Err(ParseError {
                message: format!("Expected PascalCase identifier, found {:?}", unexpected_kind),
                span: self.current_span(),
            }),
            None => Err(ParseError {
                message: "Unexpected EOF while reading PascalCase identifier".to_string(),
                span: self.current_span(),
            }),
        }
    }

    fn expect_screaming_ident(&mut self) -> Result<String, ParseError> {
        match self.peek_kind() {
            Some(TokenKind::ScreamingIdent(name)) => {
                let name_clone = name.clone();
                self.advance();
                Ok(name_clone)
            }
            Some(unexpected_kind) => Err(ParseError {
                message: format!("Expected SCREAMING_SNAKE_CASE identifier, found {:?}", unexpected_kind),
                span: self.current_span(),
            }),
            None => Err(ParseError {
                message: "Unexpected EOF while reading SCREAMING_SNAKE_CASE identifier".to_string(),
                span: self.current_span(),
            }),
        }
    }

    fn match_kind(&self, kind: &TokenKind) -> bool {
        self.peek_kind() == Some(kind)
    }

    fn peek_token(&self) -> Option<&Token> {
        if self.cursor < self.tokens.len() {
            Some(&self.tokens[self.cursor])
        } else {
            None
        }
    }

    fn peek_kind(&self) -> Option<&TokenKind> {
        self.peek_token().map(|token_ref| &token_ref.kind)
    }

    fn peek_next_kind(&self) -> Option<&TokenKind> {
        if self.cursor + 1 < self.tokens.len() {
            Some(&self.tokens[self.cursor + 1].kind)
        } else {
            None
        }
    }

    fn advance(&mut self) {
        if self.cursor < self.tokens.len() {
            self.cursor += 1;
        }
    }

    fn is_at_end(&self) -> bool {
        self.cursor >= self.tokens.len()
    }

    fn previous_span(&self) -> Span {
        if self.cursor > 0 && self.cursor - 1 < self.tokens.len() {
            self.tokens[self.cursor - 1].span
        } else if let Some(last_token) = self.tokens.last() {
            last_token.span
        } else {
            Span::new(0, 0, 1, 1)
        }
    }

    fn current_span(&self) -> Span {
        if let Some(token_ref) = self.peek_token() {
            token_ref.span
        } else if let Some(last_token) = self.tokens.last() {
            last_token.span
        } else {
            Span::new(0, 0, 1, 1)
        }
    }
}

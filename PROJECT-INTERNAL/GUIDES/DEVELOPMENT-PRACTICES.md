Here is the `DEVELOPMENT-PRACTICES.md` file, updated with the project name `dctx`.

```markdown
# DEVELOPMENT-PRACTICES Document

---

**FACTORY OF ELECTRONIC UNITS AND LOGIC – TEJL d.o.o.**
Board for Standardization and Development

  **Class:** 003-05/25-01
  **Reference Number:** 251-01-03-25-03 (Rev. 3)
  **Date:** 31.12.2025.

  SUBJECT: Technical Regulations on Concrete Development Practices for dctx
  Technical Annex to the MDAAI Protocol

---

## **Article 1: Purpose**

  (1) These regulations establish the binding technical practices for the development, execution, and maintenance of the project dctx.

  (2) The purpose of these regulations is to codify solutions for observed problems, prevent future regressions, and ensure the highest level of consistency, maintainability, and code quality.

  (3) The provisions of these regulations are complementary to and as binding as the "Protocol on the Methodology of Development with the Assistance of Artificial Intelligence (MDAAI)" itself.

## **Article 2: Relationship to Architecture Decision Records (ADRs) (NEW)**

  (1) **Scope Distinction:** This document codifies the **"how"**—specific, concrete technical solutions and best practices for recurring problems. In contrast, Architecture Decision Records (ADRs), located in `crates/dctx-core/docs/`, codify the **"why"**—the rationale behind significant, high-level architectural choices.

  (2) **Workflow:** An ADR is created to decide on a major architectural path (e.g., choosing a database technology). Once that path is implemented, if specific, recurring technical challenges or patterns emerge, their solutions are then formalized as new articles in *this* document.

---

[**Instruction for the AI Assistant:** This document is your responsibility. In accordance with the "MDAAI Protocol," you are obliged to proactively identify recurring problems or inefficient patterns during development. When such a pattern is identified and resolved, it is your duty to formalize the solution by adding a new article to this document. Before proposing a new practice, consider if the issue is a high-level architectural choice that requires an ADR instead. Maintaining this document is key to the long-term quality and efficiency of the project.]

---

**(TEMPLATE FOR NEW ARTICLES - COPY AND FILL BELOW)**

## **Article X: [Title of the specific practice]**

(1) **Problem:**
`[A clear and concise description of the recurring technical problem, error, or inefficiency that was observed.]`

(2) **Binding Solution:**
`[A precise technical description of the mandated solution. This should be the standardized way to handle the problem going forward.]`

(3) **Implementation Procedure / Prohibition:**
`[Provide specific code examples, commands, or explicit prohibitions. For example: "PROHIBITION: Direct database calls from request handlers are forbidden." or "PROCEDURE: All API responses must be formatted using the `api.SuccessResponse()` helper function."]`
```

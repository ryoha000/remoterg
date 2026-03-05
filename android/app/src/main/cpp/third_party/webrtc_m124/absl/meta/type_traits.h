#ifndef ABSL_META_TYPE_TRAITS_H_
#define ABSL_META_TYPE_TRAITS_H_

#include <type_traits>

namespace absl {

template <bool B, class T = void>
using enable_if_t = typename std::enable_if<B, T>::type;

template <class... Ts>
using void_t = void;

template <class T>
using remove_reference_t = typename std::remove_reference<T>::type;

template <class T>
using remove_const_t = typename std::remove_const<T>::type;

template <class T>
using remove_cv_t = typename std::remove_cv<T>::type;

template <class T>
using decay_t = typename std::decay<T>::type;

template <class T, class U>
using is_same = std::is_same<T, U>;

template <class T, class U>
inline constexpr bool is_same_v = std::is_same<T, U>::value;

template <class T, class U>
using is_convertible = std::is_convertible<T, U>;

template <class T, class U>
inline constexpr bool is_convertible_v = std::is_convertible<T, U>::value;

template <class T>
using is_abstract = std::is_abstract<T>;

template <class T>
inline constexpr bool is_abstract_v = std::is_abstract<T>::value;

template <class T>
using is_trivially_copyable = std::is_trivially_copyable<T>;

template <class T>
inline constexpr bool is_trivially_copyable_v = std::is_trivially_copyable<T>::value;

template <class T>
using underlying_type = std::underlying_type<T>;

template <class T>
using underlying_type_t = typename std::underlying_type<T>::type;

template <class... Ts>
using conjunction = std::conjunction<Ts...>;

template <class... Ts>
using disjunction = std::disjunction<Ts...>;

template <class T>
using negation = std::negation<T>;

template <class T>
using remove_cvref_t =
    typename std::remove_cv<typename std::remove_reference<T>::type>::type;

}  // namespace absl

#endif  // ABSL_META_TYPE_TRAITS_H_
